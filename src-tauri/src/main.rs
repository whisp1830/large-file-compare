// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gxhash::{GxHasher, HashMap, HashMapExt};
use memmap2::Mmap;
use std::fs::File;
use std::hash::Hasher;
use std::io::{BufRead, BufReader, Error as IoError, Seek, SeekFrom};
use std::thread;
use tauri::{AppHandle, Emitter};
use rayon::prelude::*;


// --- Data Structures for Frontend Communication ---

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    percentage: f64,
    file: String,
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct UniqueLinePayload {
    file: String,
    line_number: usize,
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct StepDetailPayload {
    step: String,
    duration_ms: u128,
}

#[derive(Clone, serde::Serialize)]
struct ComparisonFinishedPayload {}

fn hash_line(line: &str) -> u64 {
    let mut hasher = GxHasher::default();
    // 将字符串的字节写入哈希器。
    hasher.write(line.as_bytes());
    // 完成哈希计算并返回结果。
    hasher.finish()
}

fn find_newline_positions_parallel(mmap: &Mmap) -> Vec<usize> {
    const CHUNK_SIZE: usize = 16 * 1024 * 1024;

    let mut positions: Vec<usize> = mmap
        .par_chunks(CHUNK_SIZE)
        .enumerate()
        .flat_map(|(chunk_index, chunk)| {
            let base_offset = chunk_index * CHUNK_SIZE;

            // 1. 在当前块中串行查找，并计算出全局位置。
            let local_positions: Vec<usize> = memchr::memchr_iter(b'\n', chunk)
                .map(|local_pos| base_offset + local_pos)
                .collect(); // 2. 将结果收集到一个临时的 Vec 中。

            // 3. 将这个 Vec 转换为并行迭代器。
            //    因为 Vec 是可以被分割的，所以这个操作是有效的。
            local_positions.into_par_iter()
        })
        .collect(); // 这里会把所有块产生的位置收集到最终的 Vec 中

    // 并行收集的结果是无序的，需要排序
    positions.par_sort_unstable();

    positions
}

// --- Pass 1: 生成哈希计数和索引 (并行版) ---
// 使用 rayon 和 memmap 并行处理文件行，以提高多核CPU利用率。
fn generate_hash_counts_and_index(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<(HashMap<u64, usize>, HashMap<u64, (u64, usize)>), IoError> {
    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok((HashMap::new(), HashMap::new()));
    }

    // Emit initial progress
    if let Err(e) = app.emit("progress", ProgressPayload { percentage: 0.0, file: progress_file_id.to_string(), text: format!("Hashing file {}...", progress_file_id) }) {
        eprintln!("Failed to emit progress for File {}: {}", progress_file_id, e);
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // Find all line endings to define our work units
    let newline_positions: Vec<usize> = find_newline_positions_parallel(&mmap);
    let total_lines = newline_positions.len();

    // Process all full lines in parallel
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

    // Handle the remainder of the file (the part after the last newline)
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
    
    Ok((line_counts, line_index))
}


// --- Pass 2: 根据唯一的哈希值和索引收集行文本 ---
// 这个函数接收一个包含唯一哈希和计数的Map，以及一个从哈希到（偏移量，行号）的索引。
// 它使用索引直接跳转到文件中的特定位置来读取行，避免了全文件扫描。
// This function receives a map of unique hashes and their counts, and an index from hash to (offset, line number).
// It uses the index to jump directly to specific locations in the file to read lines, avoiding a full file scan.
fn collect_unique_lines_with_index(
    app: &AppHandle,
    file_path: &str,
    unique_hashes: HashMap<u64, usize>,
    hash_to_info: &HashMap<u64, (u64, usize)>,
    file_id: &str,
) -> Result<(), IoError> {
    if unique_hashes.is_empty() {
        return Ok(())
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
                format!("{} (x{})", line_str, count)
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


// --- Tauri Command: 没有变化 ---
#[tauri::command]
async fn start_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
) -> Result<(), String> {
    thread::spawn(move || {
        if let Err(e) = run_comparison(app, file_a_path, file_b_path) {
            // Handle errors, maybe emit an event to the frontend
            eprintln!("Comparison failed: {}", e);
        }
    });
    Ok(())
}

// --- Main Comparison Logic ---

fn run_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
) -> Result<(), std::io::Error> {
    let start_time = std::time::Instant::now();

    // --- Step 1: 并行处理两个文件，生成哈希计数和索引 ---
    let app_a = app.clone();
    let path_a_clone = file_a_path.clone();
    let handle_a = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = generate_hash_counts_and_index(&app_a, &path_a_clone, "A");
        (result, now.elapsed().as_millis())
    });

    let app_b = app.clone();
    let path_b_clone = file_b_path.clone();
    let handle_b = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = generate_hash_counts_and_index(&app_b, &path_b_clone, "B");
        (result, now.elapsed().as_millis())
    });

    // 等待线程完成并获取计数的HashMap和索引
    let (res_a, pass1_a_ms) = handle_a.join().unwrap();
    app.emit("step_completed", StepDetailPayload {
        step: "Pass 1 (File A)".to_string(),
        duration_ms: pass1_a_ms,
    }).unwrap();

    let (res_b, pass1_b_ms) = handle_b.join().unwrap();
    app.emit("step_completed", StepDetailPayload {
        step: "Pass 1 (File B)".to_string(),
        duration_ms: pass1_b_ms,
    }).unwrap();

    let (map_a_counts, index_a) = res_a?;
    let (map_b_counts, index_b) = res_b?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "A".to_string(), text: "Comparing Hashes".to_string() }).unwrap();
    println!("Pass 1: Complete.");


    // --- 中间步骤: 比较哈希计数，找出独有的哈希 ---
    let now = std::time::Instant::now();
    println!("Comparing hash maps...");
    let mut unique_to_a_counts: HashMap<u64, usize> = HashMap::new();
    let mut unique_to_b_counts: HashMap<u64, usize> = HashMap::new();

    // Iterate through File A's hashes to find differences
    for (hash, &count_a) in &map_a_counts {
        match map_b_counts.get(hash) {
            Some(&count_b) => {
                // Hash exists in both. Check if A has more.
                if count_a > count_b {
                    unique_to_a_counts.insert(*hash, count_a - count_b);
                }
            }
            None => {
                // Hash only exists in A.
                unique_to_a_counts.insert(*hash, count_a);
            }
        }
    }

    // Iterate through File B's hashes to find what's unique or more frequent in B
    for (hash, &count_b) in &map_b_counts {
        match map_a_counts.get(hash) {
            Some(&count_a) => {
                // Hash exists in both. Check if B has more.
                if count_b > count_a {
                    unique_to_b_counts.insert(*hash, count_b - count_a);
                }
            }
            None => {
                // Hash only exists in B.
                unique_to_b_counts.insert(*hash, count_b);
            }
        }
    }
    let hash_map_comparison_ms = now.elapsed().as_millis();
    app.emit("step_completed", StepDetailPayload {
        step: "Hash Map Comparison".to_string(),
        duration_ms: hash_map_comparison_ms,
    }).unwrap();
    println!("Comparison complete.");


    // --- PASS 2: 并行根据唯一的哈希和索引取回行文本 ---
    println!("Pass 2: Collecting unique lines...");
    let app_a_collect = app.clone();
    let handle_collect_a = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = collect_unique_lines_with_index(&app_a_collect, &file_a_path, unique_to_a_counts, &index_a, "A");
        (result, now.elapsed().as_millis())
    });

    let app_b_collect = app.clone();
    let handle_collect_b = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = collect_unique_lines_with_index(&app_b_collect, &file_b_path, unique_to_b_counts, &index_b, "B");
        (result, now.elapsed().as_millis())
    });

    let (res_a, pass2_a_ms) = handle_collect_a.join().unwrap();
    app.emit("step_completed", StepDetailPayload {
        step: "Pass 2 (File A)".to_string(),
        duration_ms: pass2_a_ms,
    }).unwrap();

    let (res_b, pass2_b_ms) = handle_collect_b.join().unwrap();
    app.emit("step_completed", StepDetailPayload {
        step: "Pass 2 (File B)".to_string(),
        duration_ms: pass2_b_ms,
    }).unwrap();

    res_a?;
    res_b?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "B".to_string(), text: "Comparison Finished".to_string() }).unwrap();
    println!("Pass 2: Complete.");

    // --- 最后一步: 发送最终结果 ---
    println!("Emitting final results...");
    if let Err(e) = app.emit("comparison_finished", ComparisonFinishedPayload {})
     {
        eprintln!("Failed to emit comparison_finished event: {}", e);
    }
    println!("All done in {}ms.", start_time.elapsed().as_millis());

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_comparison])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
