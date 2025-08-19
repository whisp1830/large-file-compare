use crate::payloads::{StepDetailPayload, UniqueLinePayload};
use extsort::Sortable;
use gxhash::GxHasher;
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::{File, OpenOptions};
use std::hash::Hasher;
use std::io::{BufWriter, Error as IoError, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use crate::CompareConfig;

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
/// Returns a vector of newline positions for later use.
pub fn partition_file(
    app: &AppHandle,
    input_path: &str,
    output_dir: &Path,
    progress_file_id: &str,
) -> Result<(),  IoError> {
    let total_start = Instant::now();
    emit_step_detail(app, progress_file_id, "Partitioning Started", 0);

    // --- Setup ---
    let file = File::open(input_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(());
    }
    let mmap = unsafe { Mmap::map(&file)? };
    std::fs::create_dir_all(output_dir)?;

    // --- Find lines ---
    let now = Instant::now();
    let newline_positions = find_newline_positions_parallel(&mmap);
    emit_step_detail(app, progress_file_id, "Found Newlines", now.elapsed().as_millis());

    // --- Parallel Partitioning and Writing ---
    // Create a writer for each partition file, protected by a Mutex for thread-safe access.
    let now = Instant::now();
    let writers: Vec<_> = (0..NUM_PARTITIONS)
        .map(|i| {
            let part_path = output_dir.join(format!("part_{}", i));
            let file = OpenOptions::new().write(true).create(true).truncate(true).open(part_path)?;
            Ok(Mutex::new(BufWriter::with_capacity(4 * 1024 * 1024, file)))
        })
        .collect::<Result<Vec<_>, IoError>>()?;

    // Iterate over lines in parallel, hash them, and write (hash, offset) to the correct partition file.
    // This avoids collecting all HashOffsets in memory.
    (0..newline_positions.len())
        .into_par_iter()
        .try_for_each(|i| -> Result<(), IoError> {
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

                // Lock the writer for the target partition and write the encoded data.
                let mut writer_guard = writers[partition_index].lock().unwrap();
                HashOffset(hash, offset).encode(&mut *writer_guard)?;
            }
            Ok(())
        })?;
    emit_step_detail(app, progress_file_id, "Hashing and Writing Partitions", now.elapsed().as_millis());

    emit_step_detail(app, progress_file_id, "Total Partitioning Time", total_start.elapsed().as_millis());
    Ok(())
}



pub fn collect_unique_lines(
    app: &AppHandle,
    file_path: &str,
    unique_offsets: &[(u64, usize)], // List of (offset, count)
    compare_config: &CompareConfig,
    file_id: &str,
) -> Result<(), IoError> {
    let now = Instant::now();
    if unique_offsets.is_empty() {
        return Ok(())
    }

    let file = File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let mut sorted_unique_offsets = unique_offsets.to_vec();
    sorted_unique_offsets.sort_unstable_by_key(|k| k.0);

    for (offset, count) in sorted_unique_offsets {
        let current_offset = offset as usize;

        // Efficiently find the line number using a binary search on the newline positions.
        // `partition_point` is faster than `binary_search` for this purpose.
        // It finds the index of the first newline position greater than the current offset.

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
                text: display_line,
                line_number: 0,
            },
        ) {
            eprintln!("Failed to emit unique_line event: {}", e);
        }
    }

    emit_step_detail(app, file_id, "Collecting Unique Lines", now.elapsed().as_millis());
    Ok(())
}