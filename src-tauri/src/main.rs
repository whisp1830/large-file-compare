// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gxhash::{GxHasher, HashMap, HashMapExt};
use memchr::memchr_iter;
use memmap2::Mmap;
use std::fs::File;
use std::hash::Hasher;
use std::io::Error as IoError;
use std::thread;
use tauri::{AppHandle, Emitter};


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
struct TimeCost {
    pass1_a_ms: u128,
    pass1_b_ms: u128,
    hash_map_comparison_ms: u128,
    pass2_a_ms: u128,
    pass2_b_ms: u128,
}

#[derive(Clone, serde::Serialize)]
struct ComparisonFinishedPayload {
    time_cost: TimeCost,
}

// --- 核心哈希逻辑没有变化 ---

fn hash_line(line: &str) -> u64 {
    let mut hasher = GxHasher::default();
    // 将字符串的字节写入哈希器。
    hasher.write(line.as_bytes());
    // 完成哈希计算并返回结果。
    hasher.finish()
}

// --- Pass 1: 生成哈希计数 ---
// 这个函数只计算哈希和它们的出现次数，不存储完整的行字符串，以节省内存。
fn generate_hash_counts(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<HashMap<u64, usize>, IoError> {
    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(HashMap::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // 预估容量以提高性能
    let estimated_lines = (file_size / 50).max(1024) as usize;
    let mut line_hashes: HashMap<u64, usize> = HashMap::with_capacity(estimated_lines);

    let mut bytes_processed: u64 = 0;
    let mut last_emitted_percentage: f64 = -1.0;

    for line_bytes in mmap.split(|&b| b == b'\n') {
        bytes_processed += line_bytes.len() as u64 + 1;

        if line_bytes.is_empty() {
            continue;
        }

        let line_bytes = if line_bytes.last() == Some(&b'\r') {
            &line_bytes[..line_bytes.len() - 1]
        } else {
            line_bytes
        };

        if let Ok(line_str) = std::str::from_utf8(line_bytes) {
            let hash = hash_line(line_str);
            // 直接增加计数器
            *line_hashes.entry(hash).or_insert(0) += 1;
        }

        let percentage = (bytes_processed as f64 / file_size as f64) * 100.0;
        if percentage - last_emitted_percentage >= 5.0 || percentage >= 99.9999999999999 {
            if let Err(e) = app.emit("progress", ProgressPayload { percentage, file: progress_file_id.to_string(), text: format!("Processing file {}...", progress_file_id) }) {
                eprintln!("Failed to emit progress for File {}: {}", progress_file_id, e);
            }
            last_emitted_percentage = percentage;
        }
    }

    Ok(line_hashes)
}

// --- Pass 2: 根据唯一的哈希值收集行文本 ---
// 这个函数接收一个包含唯一哈希和计数的Map，然后再次读取文件，把对应的行文本找出来。
fn collect_unique_lines(
    app: &AppHandle,
    file_path: &str,
    mut unique_hashes: HashMap<u64, usize>,
    file_id: &str,
) -> Result<(), IoError> {
    // 初始检查
    if unique_hashes.is_empty() {
        return Ok(());
    }

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // 1. 快速查找所有换行符的位置（与原版相同，高效）
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(memchr_iter(b'\n', &mmap).map(|i| i + 1))
        .collect();

    // 2. 顺序处理每一行
    for (i, &start) in line_starts.iter().enumerate() {
        // 精确早停：如果所有唯一哈希已找到，停止扫描剩余行
        if unique_hashes.is_empty() {
            break;
        }

        // 计算行在 mmap 中的字节范围
        let end = if i + 1 < line_starts.len() {
            line_starts[i + 1] - 1 // 到下一个换行符之前
        } else {
            mmap.len() // 文件末尾
        };

        // 如果 start 越界或行为空，则跳过
        if start >= end {
            continue;
        }

        let mut line_bytes = &mmap[start..end];
        if line_bytes.last() == Some(&b'\r') {
            line_bytes = &line_bytes[..line_bytes.len() - 1];
        }

        // 转换为 UTF-8 并计算哈希
        if let Ok(line_str) = std::str::from_utf8(line_bytes) {
            let hash = hash_line(line_str);

            // 检查并移除（单线程，无需锁）
            if let Some(count) = unique_hashes.remove(&hash) {
                let display_line = if count > 1 {
                    format!("{} (x{})", line_str, count)
                } else {
                    line_str.to_string()
                };
                if let Err(e) = app.emit("unique_line", UniqueLinePayload {
                    file: file_id.to_string(),
                    line_number: i + 1,
                    text: display_line,
                }) {
                    eprintln!("Failed to emit unique_line event: {}", e);
                }
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

    // --- Step 1: 并行处理两个文件 ---
    let app_a = app.clone();
    let path_a_clone = file_a_path.clone();
    let handle_a = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = generate_hash_counts(&app_a, &path_a_clone, "A");
        (result, now.elapsed().as_millis())
    });

    let app_b = app.clone();
    let path_b_clone = file_b_path.clone();
    let handle_b = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = generate_hash_counts(&app_b, &path_b_clone, "B");
        (result, now.elapsed().as_millis())
    });

    // 等待线程完成并获取计数的HashMap
    let (map_a_counts_res, pass1_a_ms) = handle_a.join().unwrap();
    let (map_b_counts_res, pass1_b_ms) = handle_b.join().unwrap();
    let map_a_counts = map_a_counts_res?;
    let map_b_counts = map_b_counts_res?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "A".to_string(), text: "Comparing Hashes".to_string() }).unwrap();
    println!("Pass 1: Complete.");


    // --- 中间步骤: 比较哈希计数，找出独有的哈希 ---
    let now = std::time::Instant::now();
    println!("Comparing hash maps...");
    let mut unique_to_a_counts: HashMap<u64, usize> = HashMap::new();
    let mut unique_to_b_counts: HashMap<u64, usize> = HashMap::new();

    // 找出文件A中独有的或多出的行
    for (hash, count_a) in map_a_counts.iter() {
        match map_b_counts.get(hash) {
            Some(count_b) => {
                if count_a > count_b {
                    unique_to_a_counts.insert(*hash, count_a - count_b);
                }
            }
            None => {
                unique_to_a_counts.insert(*hash, *count_a);
            }
        }
    }

    // 找出文件B中独有的或多出的行
    for (hash, count_b) in map_b_counts.iter() {
        if !map_a_counts.contains_key(hash) {
            unique_to_b_counts.insert(*hash, *count_b);
        }
    }
    let hash_map_comparison_ms = now.elapsed().as_millis();
    println!("Comparison complete.");


    // --- PASS 2: 并行根据唯一的哈希取回行文本 ---
    println!("Pass 2: Collecting unique lines...");
    let app_a_collect = app.clone();
    let handle_collect_a = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = collect_unique_lines(&app_a_collect, &file_a_path, unique_to_a_counts, "A");
        (result, now.elapsed().as_millis())
    });

    let app_b_collect = app.clone();
    let handle_collect_b = thread::spawn(move || {
        let now = std::time::Instant::now();
        let result = collect_unique_lines(&app_b_collect, &file_b_path, unique_to_b_counts, "B");
        (result, now.elapsed().as_millis())
    });

    let (res_a, pass2_a_ms) = handle_collect_a.join().unwrap();
    let (res_b, pass2_b_ms) = handle_collect_b.join().unwrap();
    res_a?;
    res_b?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "B".to_string(), text: "Comparison Finished".to_string() }).unwrap();
    println!("Pass 2: Complete.");

    // --- 最后一步: 发送最终结果 ---
    println!("Emitting final results...");
    let time_cost = TimeCost {
        pass1_a_ms,
        pass1_b_ms,
        hash_map_comparison_ms,
        pass2_a_ms,
        pass2_b_ms,
    };
    if let Err(e) = app.emit("comparison_finished", ComparisonFinishedPayload { time_cost }) {
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