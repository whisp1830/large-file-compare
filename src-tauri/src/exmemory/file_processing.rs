use crate::payloads::{StepDetailPayload, UniqueLinePayload};
use extsort::Sortable;
use gxhash::GxHasher;
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::{File, OpenOptions};
use std::hash::Hasher;
use std::io::{BufWriter, Error as IoError, Read, Write};
use std::path::Path;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

// Helper to emit step details to the frontend
fn emit_step_detail(app: &AppHandle, file_id: &str, step_name: &str, duration_ms: u128) {
    let step_label = format!("File {} - {}", file_id, step_name);
    if let Err(e) = app.emit(
        "step_completed",
        StepDetailPayload {
            step: step_label,
            duration_ms,
        },
    ) {
        eprintln!("Failed to emit step_completed event: {}", e);
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub struct HashOffset(pub u64, pub u64);

// We keep the Sortable trait implementation as it provides a convenient
// binary encoding/decoding format for writing to our partition files.
impl Sortable for HashOffset {
    fn encode<W: Write>(&self, writer: &mut W) -> Result<(), IoError> {
        writer.write_all(&self.0.to_le_bytes())?;
        writer.write_all(&self.1.to_le_bytes())?;
        Ok(())
    }

    fn decode<R: Read>(reader: &mut R) -> Result<Self, IoError> {
        let mut hash_bytes = [0u8; 8];
        reader.read_exact(&mut hash_bytes)?;
        let mut offset_bytes = [0u8; 8];
        reader.read_exact(&mut offset_bytes)?;
        Ok(HashOffset(
            u64::from_le_bytes(hash_bytes),
            u64::from_le_bytes(offset_bytes),
        ))
    }
}

fn hash_line(line: &[u8]) -> u64 {
    let mut hasher = GxHasher::default();
    hasher.write(line);
    hasher.finish()
}

fn find_newline_positions_parallel(mmap: &Mmap) -> Vec<usize> {
    const CHUNK_SIZE: usize = 16 * 1024 * 1024;

    let mmap_ptr = mmap.as_ptr() as usize;
    let list_of_vectors: Vec<Vec<usize>> = mmap.par_chunks(CHUNK_SIZE)
        .map(|chunk| {
            let chunk_start_offset = chunk.as_ptr() as usize - mmap_ptr;
            memchr::memchr_iter(b'\n', chunk)
                .map(move |pos| chunk_start_offset + pos)
                .collect::<Vec<_>>()
        })
        .collect();

    let total_positions = list_of_vectors.iter().map(|v| v.len()).sum();
    let mut result = Vec::with_capacity(total_positions);
    for vec in list_of_vectors {
        result.extend(vec);
    }

    result
}

pub const NUM_PARTITIONS: u64 = 256;

/// Partitions a file into smaller files based on the hash of each line.
pub fn partition_file(
    app: &AppHandle,
    input_path: &str,
    output_dir: &Path,
    progress_file_id: &str,
) -> Result<(), IoError> {
    let total_start = Instant::now();
    emit_step_detail(app, progress_file_id, "Partitioning Started", 0);

    // --- Setup ---
    let file = File::open(input_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(())
    }
    let mmap = unsafe { Mmap::map(&file)? };
    std::fs::create_dir_all(output_dir)?;

    // --- Find lines ---
    let now = Instant::now();
    let mut newline_positions = find_newline_positions_parallel(&mmap);
    emit_step_detail(app, progress_file_id, "Found & Sorted Newlines", now.elapsed().as_millis());

    // --- Parallel Partitioning ---
    // Each thread will create a vector of HashOffsets for each partition.
    // This avoids lock contention on file writers.
    let now = Instant::now();
    let partitioned_data: Vec<Vec<HashOffset>> = (0..newline_positions.len())
        .into_par_iter()
        .fold(
            || vec![Vec::new(); NUM_PARTITIONS as usize],
            |mut thread_local_partitions, i| {
                let start = if i == 0 { 0 } else { newline_positions[i - 1] + 1 };
                let end = newline_positions[i];
                let line_bytes = &mmap[start..end];
                let line_bytes_cleaned = if line_bytes.last() == Some(&b'\r') {
                    &line_bytes[..line_bytes.len() - 1]
                } else {
                    line_bytes
                };

                if !line_bytes_cleaned.is_empty() {
                    let hash = hash_line(line_bytes_cleaned);
                    let offset = start as u64;
                    let partition_index = (hash % NUM_PARTITIONS) as usize;
                    thread_local_partitions[partition_index].push(HashOffset(hash, offset));
                }
                thread_local_partitions
            },
        )
        .reduce(
            || vec![Vec::new(); NUM_PARTITIONS as usize],
            |mut combined_partitions, thread_local_partitions| {
                for (i, part_data) in thread_local_partitions.into_iter().enumerate() {
                    combined_partitions[i].extend(part_data);
                }
                combined_partitions
            },
        );
    emit_step_detail(app, progress_file_id, "In-Memory Partitioning", now.elapsed().as_millis());


    // --- Write Partitions to Disk ---
    // This is done sequentially per partition to maximize write performance.
    let now = Instant::now();
    partitioned_data
        .into_par_iter()
        .enumerate()
        .try_for_each(|(i, data)| -> Result<(), IoError> {
            if data.is_empty() {
                return Ok(())
            }
            let part_path = output_dir.join(format!("part_{}", i));
            let file = OpenOptions::new().write(true).create(true).truncate(true).open(part_path)?;
            let mut writer = BufWriter::new(file);
            for item in data {
                item.encode(&mut writer)?;
            }
            writer.flush()?;
            Ok(())
        })?;
    emit_step_detail(app, progress_file_id, "Writing Partitions to Disk", now.elapsed().as_millis());

    emit_step_detail(app, progress_file_id, "Total Partitioning Time", total_start.elapsed().as_millis());
    Ok(())
}


pub fn collect_unique_lines(
    app: &AppHandle,
    file_path: &str,
    unique_offsets: &[(u64, usize)], // List of (offset, count)
    file_id: &str,
) -> Result<(), IoError> {
    if unique_offsets.is_empty() {
        return Ok(())
    }

    let file = File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let mut sorted_unique_offsets = unique_offsets.to_vec();
    sorted_unique_offsets.sort_unstable_by_key(|k| k.0);

    let mut last_scan_offset: usize = 0;
    let mut last_line_number: usize = 0;

    for (offset, count) in sorted_unique_offsets {
        let current_offset = offset as usize;

        // This logic is slow if offsets are far apart.
        // A better way would be to find the line number from the offset directly
        // if we had a pre-computed index of newline positions.
        // For now, we keep it simple.
        let newlines_in_between =
            memchr::memchr_iter(b'\n', &mmap[last_scan_offset..current_offset]).count();
        let line_number = last_line_number + newlines_in_between + 1;

        let line_end = memchr::memchr(b'\n', &mmap[current_offset..])
            .map_or(mmap.len(), |pos| current_offset + pos);

        let line_bytes = &mmap[current_offset..line_end];
        let line_str = String::from_utf8_lossy(line_bytes).trim_end().to_string();

        let display_line = if count > 1 {
            format!("{}\n(x{})", line_str, count)
        } else {
            line_str
        };

        if let Err(e) = app.emit(
            "unique_line",
            UniqueLinePayload {
                file: file_id.to_string(),
                line_number,
                text: display_line,
            },
        ) {
            eprintln!("Failed to emit unique_line event: {}", e);
        }

        last_scan_offset = current_offset;
        last_line_number = line_number - 1;
    }

    Ok(())
}