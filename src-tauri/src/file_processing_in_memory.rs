use crate::payloads::{ProgressPayload, StepDetailPayload, UniqueLinePayload};
use gxhash::{GxHasher, HashMap, HashMapExt};
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::File;
use std::hash::Hasher;
use std::io::{Error as IoError};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

// Helper to emit step details to the frontend
fn emit_step_detail(app: &AppHandle, file_id: &str, step_name: &str, duration_ms: u128) {
    let step_label = format!("File {} - {}", file_id, step_name);
    if let Err(e) = app.emit("step_completed", StepDetailPayload {
        step: step_label,
        duration_ms,
    }) {
        eprintln!("Failed to emit step_completed event: {}", e);
    }
}

fn hash_line(line: &[u8]) -> u64 {
    let mut hasher = GxHasher::default();
    hasher.write(line);
    hasher.finish()
}

// Single-pass hash generation.
pub fn generate_hash_counts_and_index(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<(HashMap<u64, usize>, HashMap<u64, u64>), IoError> {
    let total_start = Instant::now();

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    emit_step_detail(app, progress_file_id, "Opened file & read metadata", total_start.elapsed().as_millis());

    if file_size == 0 {
        return Ok((HashMap::new(), HashMap::new()));
    }

    let _ = app.emit("progress", ProgressPayload { percentage: 0.0, file: progress_file_id.to_string(), text: format!("Hashing file {}...", progress_file_id) });

    let mmap = unsafe { Mmap::map(&file)? };
    emit_step_detail(app, progress_file_id, "Created memory map", total_start.elapsed().as_millis());

    let now = Instant::now();
    const CHUNK_SIZE: usize = 16 * 1024 * 1024; // 16MB

    // Process chunks in parallel
    let chunk_results: Vec<_> = mmap.par_chunks(CHUNK_SIZE).enumerate().map(|(chunk_idx, chunk)| {
        let mut local_results = Vec::new();
        let chunk_start_offset = (chunk_idx * CHUNK_SIZE) as u64;
        let mut last_pos = 0;

        for nl_pos in memchr::memchr_iter(b'\n', chunk) {
            let offset = chunk_start_offset + last_pos as u64;
            let line_bytes = &chunk[last_pos..nl_pos];
            let line_bytes_cleaned = if line_bytes.last() == Some(&b'\r') { &line_bytes[..line_bytes.len() - 1] } else { line_bytes };

            if !line_bytes_cleaned.is_empty() {
                local_results.push((hash_line(line_bytes_cleaned), offset));
            }
            last_pos = nl_pos + 1;
        }

        // Return processed lines and the remainder of the chunk
        (local_results, &chunk[last_pos..])
    }).collect();
    emit_step_detail(app, progress_file_id, "Parallel chunk processing", now.elapsed().as_millis());


    let now = Instant::now();
    // Stitch together remainders and process them
    let mut stitched_lines = Vec::new();
    let mut line_buffer = Vec::new();
    let mut last_chunk_len = 0;

    for (i, (_lines, remainder)) in chunk_results.iter().enumerate() {
        let chunk_start_offset = (i * CHUNK_SIZE) as u64;
        if !line_buffer.is_empty() {
            // Find newline in the current remainder
            if let Some(nl_pos) = memchr::memchr(b'\n', remainder) {
                line_buffer.extend_from_slice(&remainder[..nl_pos]);
                let offset = chunk_start_offset - last_chunk_len as u64 + line_buffer.iter().position(|&b| b != 0).unwrap_or(0) as u64;
                let cleaned_buffer = if line_buffer.last() == Some(&b'\r') { &line_buffer[..line_buffer.len() - 1] } else { &line_buffer[..] };
                if !cleaned_buffer.is_empty() {
                    stitched_lines.push((hash_line(cleaned_buffer), offset));
                }
                line_buffer.clear();
            } else {
                line_buffer.extend_from_slice(remainder);
            }
        }
        // Start new buffer with the current remainder if it's not empty
        if !remainder.is_empty() && memchr::memchr(b'\n', remainder).is_none() {
            line_buffer.extend_from_slice(remainder);
            last_chunk_len = remainder.len();
        }
    }

    // Process final remainder if any
    if !line_buffer.is_empty() {
        let offset = file_size - line_buffer.len() as u64;
        let cleaned_buffer = if line_buffer.last() == Some(&b'\r') { &line_buffer[..line_buffer.len() - 1] } else { &line_buffer[..] };
        if !cleaned_buffer.is_empty() {
            stitched_lines.push((hash_line(cleaned_buffer), offset));
        }
    }
    emit_step_detail(app, progress_file_id, "Stitched chunk remainders", now.elapsed().as_millis());


    let now = Instant::now();
    // Aggregate results
    let mut line_counts: HashMap<u64, usize> = HashMap::new();
    let mut line_index: HashMap<u64, u64> = HashMap::new();

    let all_lines = chunk_results.into_iter().flat_map(|(lines, _)| lines).chain(stitched_lines);

    for (hash, offset) in all_lines {
        *line_counts.entry(hash).or_insert(0) += 1;
        line_index.entry(hash).or_insert(offset);
    }
    emit_step_detail(app, progress_file_id, "Aggregated results", now.elapsed().as_millis());
    emit_step_detail(app, progress_file_id, "Total Hashing/Indexing Time", total_start.elapsed().as_millis());

    Ok((line_counts, line_index))
}


pub fn collect_unique_lines_with_index(
    app: &AppHandle,
    file_path: &str,
    unique_hashes: HashMap<u64, usize>,
    hash_to_offset: &HashMap<u64, u64>,
    file_id: &str,
) -> Result<(), IoError> {
    if unique_hashes.is_empty() {
        return Ok(());
    }

    let file = File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    // Create a sorted list of offsets to read sequentially
    let mut sorted_unique_offsets: Vec<(u64, usize)> = unique_hashes
        .iter()
        .filter_map(|(hash, count)| hash_to_offset.get(hash).map(|offset| (*offset, *count)))
        .collect();
    sorted_unique_offsets.sort_unstable_by_key(|k| k.0);

    let mut last_scan_offset: usize = 0;
    let mut last_line_number: usize = 0;

    for (offset, count) in sorted_unique_offsets {
        let current_offset = offset as usize;

        // Count newlines from the last scanned position to the current offset
        let newlines_in_between = memchr::memchr_iter(b'\n', &mmap[last_scan_offset..current_offset]).count();
        let line_number = last_line_number + newlines_in_between + 1;

        // Find the end of the current line
        let line_end = memchr::memchr(b'\n', &mmap[current_offset..])
            .map_or(mmap.len(), |pos| current_offset + pos);

        let line_bytes = &mmap[current_offset..line_end];
        let line_str = String::from_utf8_lossy(line_bytes).trim_end().to_string();

        let display_line = if count > 1 {
            format!("{}\n(x{})", line_str, count)
        } else {
            line_str
        };

        if let Err(e) = app.emit("unique_line", UniqueLinePayload {
            file: file_id.to_string(),
            line_number,
            text: display_line,
        }) {
            eprintln!("Failed to emit unique_line event: {}", e);
        }

        last_scan_offset = current_offset;
        last_line_number = line_number -1;
    }

    Ok(())
}
