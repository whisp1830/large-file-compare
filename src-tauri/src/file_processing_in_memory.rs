use crate::payloads::{ProgressPayload, StepDetailPayload, UniqueLinePayload};
use gxhash::{GxHasher, HashMap, HashMapExt};
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::File;
use std::hash::Hasher;
use std::io::{BufRead, BufReader, Error as IoError, Seek, SeekFrom};
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

fn hash_line(line: &str) -> u64 {
    let mut hasher = GxHasher::default();
    hasher.write(line.as_bytes());
    hasher.finish()
}

fn find_newline_positions_parallel(mmap: &Mmap) -> Vec<usize> {
    const CHUNK_SIZE: usize = 16 * 1024 * 1024;

    let mut positions: Vec<usize> = mmap
        .par_chunks(CHUNK_SIZE)
        .enumerate()
        .flat_map(|(chunk_index, chunk)| {
            let base_offset = chunk_index * CHUNK_SIZE;
            let local_positions: Vec<usize> = memchr::memchr_iter(b'\n', chunk)
                .map(|local_pos| base_offset + local_pos)
                .collect();
            local_positions.into_par_iter()
        })
        .collect();

    positions.par_sort_unstable();
    positions
}

pub fn generate_hash_counts_and_index(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<(HashMap<u64, usize>, HashMap<u64, (u64, usize)>), IoError> {
    let total_start = Instant::now();

    // --- File Open & Metadata ---
    let now = Instant::now();
    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    emit_step_detail(app, progress_file_id, "Opened file & read metadata", now.elapsed().as_millis());

    if file_size == 0 {
        return Ok((HashMap::new(), HashMap::new()));
    }

    if let Err(e) = app.emit("progress", ProgressPayload { percentage: 0.0, file: progress_file_id.to_string(), text: format!("Hashing file {}...", progress_file_id) }) {
        eprintln!("Failed to emit progress for File {}: {}", progress_file_id, e);
    }

    // --- Memory Map ---
    let now = Instant::now();
    let mmap = unsafe { Mmap::map(&file)? };
    emit_step_detail(app, progress_file_id, "Created memory map", now.elapsed().as_millis());

    // --- Find Newline Positions ---
    let now = Instant::now();
    let newline_positions: Vec<usize> = find_newline_positions_parallel(&mmap);
    let total_lines = newline_positions.len();
    emit_step_detail(app, progress_file_id, "Found all newline positions", now.elapsed().as_millis());

    // --- Parallel Processing ---
    let now = Instant::now();
    let (mut line_counts, mut line_index) = if total_lines > 0 {
        (0..total_lines)
            .into_par_iter()
            .filter_map(|i| {
                let start = if i == 0 { 0 } else { newline_positions[i - 1] + 1 };
                let end = newline_positions[i];
                let line_bytes = &mmap[start..end];
                let line_bytes_cleaned = if line_bytes.last() == Some(&b'\r') {
                    &line_bytes[..line_bytes.len() - 1]
                } else {
                    line_bytes
                };
                if line_bytes_cleaned.is_empty() {
                    return None;
                }
                if let Ok(line_str) = std::str::from_utf8(line_bytes_cleaned) {
                    let hash = hash_line(line_str);
                    let offset = start as u64;
                    let line_number = i + 1;
                    Some((hash, offset, line_number))
                } else {
                    None
                }
            })
            .fold(
                || (HashMap::new(), HashMap::new()),
                |mut acc, (hash, offset, line_number)| {
                    *acc.0.entry(hash).or_insert(0) += 1;
                    acc.1.entry(hash).or_insert((offset, line_number));
                    acc
                },
            )
            .reduce(
                || (HashMap::new(), HashMap::new()),
                |mut map_a, map_b| {
                    for (hash, count_b) in map_b.0 {
                        *map_a.0.entry(hash).or_insert(0) += count_b;
                    }
                    for (hash, info_b) in map_b.1 {
                        map_a.1.entry(hash)
                            .and_modify(|info_a| {
                                if info_b.0 < info_a.0 {
                                    *info_a = info_b;
                                }
                            })
                            .or_insert(info_b);
                    }
                    map_a
                },
            )
    } else {
        (HashMap::new(), HashMap::new())
    };
    emit_step_detail(app, progress_file_id, "Processed lines in parallel (hashing, counting, indexing)", now.elapsed().as_millis());

    // --- Remainder Processing ---
    let now = Instant::now();
    let last_newline_pos = newline_positions.last().map_or(0, |p| p + 1);
    if last_newline_pos < mmap.len() {
        let remainder = &mmap[last_newline_pos..];
        let line_bytes_cleaned = if remainder.last() == Some(&b'\r') {
            &remainder[..remainder.len() - 1]
        } else {
            remainder
        };
        if !line_bytes_cleaned.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(line_bytes_cleaned) {
                let hash = hash_line(line_str);
                *line_counts.entry(hash).or_insert(0) += 1;
                line_index.entry(hash).or_insert((last_newline_pos as u64, total_lines + 1));
            }
        }
    }
    if last_newline_pos < mmap.len() {
        let remainder = &mmap[last_newline_pos..];
        if !remainder.is_empty() {
            emit_step_detail(app, progress_file_id, "Processed file remainder", now.elapsed().as_millis());
        }
    }


    emit_step_detail(app, progress_file_id, "Total Hashing/Indexing Time", total_start.elapsed().as_millis());

    Ok((line_counts, line_index))
}

pub fn collect_unique_lines_with_index(
    app: &AppHandle,
    file_path: &str,
    unique_hashes: HashMap<u64, usize>,
    hash_to_info: &HashMap<u64, (u64, usize)>,
    file_id: &str,
) -> Result<(), IoError> {
    if unique_hashes.is_empty() {
        return Ok(());
    }

    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    for (hash, count) in unique_hashes.iter() {
        if let Some((offset, line_number)) = hash_to_info.get(hash) {
            reader.seek(SeekFrom::Start(*offset))?;
            let mut line_buffer = String::new();
            reader.read_line(&mut line_buffer)?;
            let line_str = line_buffer.trim_end();
            let display_line = if *count > 1 {
                format!("{}\n(x{})", line_str, count)
            } else {
                line_str.to_string()
            };
            if let Err(e) = app.emit("unique_line", UniqueLinePayload {
                file: file_id.to_string(),
                line_number: *line_number,
                text: display_line,
            }) {
                eprintln!("Failed to emit unique_line event: {}", e);
            }
        }
    }

    Ok(())
}