use crate::payloads::{ProgressPayload, StepDetailPayload, UniqueLinePayload};
use extsort::{ExternalSorter, Sortable};
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

pub fn create_sorted_hash_file(
    app: &AppHandle,
    input_path: &str,
    output_path: &Path,
    progress_file_id: &str,
) -> Result<(), IoError> {
    let total_start = Instant::now();
    let file = File::open(input_path)?;
    let file_size = file.metadata()?.len();
    emit_step_detail(
        app,
        progress_file_id,
        "Opened file & read metadata",
        total_start.elapsed().as_millis(),
    );

    if file_size == 0 {
        File::create(output_path)?;
        return Ok(());
    }

    let _ = app.emit(
        "progress",
        ProgressPayload {
            percentage: 0.0,
            file: progress_file_id.to_string(),
            text: format!("Hashing file {}...", progress_file_id),
        },
    );

    let mmap = unsafe { Mmap::map(&file)? };
    emit_step_detail(
        app,
        progress_file_id,
        "Created memory map",
        total_start.elapsed().as_millis(),
    );

    let now = Instant::now();
    let sorter = ExternalSorter::new();

    const CHUNK_SIZE: usize = 32 * 1024 * 1024;

    let mmap_ptr = mmap.as_ptr() as usize;
    let all_items: Vec<_> = mmap.par_chunks(CHUNK_SIZE).flat_map(move |chunk| {
        let mut lines = Vec::new();
        let mut last_pos = 0;
        for nl_pos in memchr::memchr_iter(b'\n', chunk) {
            let line_bytes = &chunk[last_pos..nl_pos];
            let line_bytes_cleaned = if line_bytes.last() == Some(&b'\r') {
                &line_bytes[..line_bytes.len() - 1]
            } else {
                line_bytes
            };
            if !line_bytes_cleaned.is_empty() {
                let hash = hash_line(line_bytes_cleaned);
                let global_offset =
                    (chunk.as_ptr() as usize - mmap_ptr + last_pos) as u64;
                lines.push(HashOffset(hash, global_offset));
            }
            last_pos = nl_pos + 1;
        }
        lines
    }).collect();

    let sorted_iter = sorter.sort(all_items.into_iter()).unwrap();

    emit_step_detail(
        app,
        progress_file_id,
        "Parallel hashing and in-memory sort",
        now.elapsed().as_millis(),
    );

    let now = Instant::now();
    let output_file =
        OpenOptions::new().write(true).create(true).truncate(true).open(output_path)?;
    let mut buf_writer = BufWriter::new(output_file);
    for item in sorted_iter {
        item?.encode(&mut buf_writer)?;
    }
    emit_step_detail(
        app,
        progress_file_id,
        "External merge sort",
        now.elapsed().as_millis(),
    );

    emit_step_detail(
        app,
        progress_file_id,
        "Total Hashing/Sorting Time",
        total_start.elapsed().as_millis(),
    );

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