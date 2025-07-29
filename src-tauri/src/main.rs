// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ahash::{AHashMap, AHasher};
use memchr::memchr_iter;
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::File;
use std::hash::Hasher;
use std::io::Error as IoError;
use std::sync::Mutex;
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
struct DiffLine {
    line_number: usize,
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct DiffPayload {
    unique_to_a: Vec<DiffLine>,
    unique_to_b: Vec<DiffLine>,
}

// --- 核心哈希逻辑没有变化 ---

fn hash_line(line: &str) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(line.as_bytes());
    hasher.finish()
}

// --- Pass 1: 生成哈希计数 ---
// 这个函数只计算哈希和它们的出现次数，不存储完整的行字符串，以节省内存。
fn generate_hash_counts(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<AHashMap<u64, usize>, IoError> {
    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(AHashMap::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // 预估容量以提高性能
    let estimated_lines = (file_size / 50).max(1024) as usize;
    let mut line_hashes: AHashMap<u64, usize> = AHashMap::with_capacity(estimated_lines);

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

        // 进度报告逻辑保持不变
        let percentage = (bytes_processed as f64 / file_size as f64) * 100.0;
        if percentage - last_emitted_percentage >= 3.0 || percentage >= 99.9999999999999 {
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
    file_path: &str,
    unique_hashes: AHashMap<u64, usize>, // 注意：类型变为 AHashMap
) -> Result<Vec<DiffLine>, IoError> {
    // 初始检查与原版相同
    if unique_hashes.is_empty() {
        return Ok(Vec::new());
    }

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(Vec::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // 将 HashMap 包裹在 Mutex 中，以便在多线程中安全访问
    // Mutex 用于确保每次只有一个线程可以修改 unique_hashes
    let shared_hashes = Mutex::new(unique_hashes);

    // 1. 快速查找所有换行符的位置
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(memchr_iter(b'\n', &mmap).map(|i| i + 1))
        .collect();

    // 2. 使用 Rayon 并行处理每一行
    let mut found_lines: Vec<DiffLine> = line_starts
        .par_iter()
        .enumerate()
        .filter_map(|(i, &start)| {
            // 提前检查 shared_hashes 是否已空，减少不必要的处理和锁竞争
            if shared_hashes.lock().unwrap().is_empty() {
                return None;
            }

            // 计算行在 mmap 中的字节范围
            let end = if i + 1 < line_starts.len() {
                line_starts[i + 1] - 1 // 到下一个换行符之前
            } else {
                mmap.len() // 文件末尾
            };

            // 如果 start 越界或行为空，则跳过
            if start >= end {
                return None;
            }

            let mut line_bytes = &mmap[start..end];
            if line_bytes.last() == Some(&b'\r') {
                line_bytes = &line_bytes[..line_bytes.len() - 1];
            }

            // 只有当哈希可能存在时，才进行 UTF-8 转换和进一步处理
            if let Ok(line_str) = std::str::from_utf8(line_bytes) {
                let hash = hash_line(line_str);

                // 锁住 HashMap，执行检查和移除操作
                let mut hashes = shared_hashes.lock().unwrap();
                if let Some(count) = hashes.remove(&hash) {
                    // 如果找到了，构造 DiffLine 并返回
                    let display_line = if count > 1 {
                        format!("{} (x{})", line_str, count)
                    } else {
                        line_str.to_string()
                    };
                    return Some(DiffLine {
                        line_number: i + 1, // 行号从 1 开始
                        text: display_line,
                    });
                }
            }

            // 如果没找到，返回 None
            None
        })
        .collect(); // 从所有线程收集结果

    // 结果默认是无序的，因为并行执行。如果需要按行号排序，增加这一步。
    found_lines.par_sort_unstable_by_key(|d| d.line_number);

    Ok(found_lines)
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

    // --- Step 1: 并行处理两个文件 ---
    let app_a = app.clone();
    let path_a_clone = file_a_path.clone();
    let handle_a = thread::spawn(move || {
        generate_hash_counts(&app_a, &path_a_clone, "A")
    });

    let app_b = app.clone();
    let path_b_clone = file_b_path.clone();
    let handle_b = thread::spawn(move || {
        generate_hash_counts(&app_b, &path_b_clone, "B")
    });

    // 等待线程完成并获取计数的HashMap
    // .unwrap() 会在线程 panic 时 panic，生产代码中应使用更稳健的错误处理
    let map_a_counts = handle_a.join().unwrap()?;
    let map_b_counts = handle_b.join().unwrap()?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "A".to_string(), text: "Comparing Hashes".to_string() }).unwrap();
    println!("Pass 1: Complete.");


    // --- 中间步骤: 比较哈希计数，找出独有的哈希 ---
    println!("Comparing hash maps...");
    let mut unique_to_a_counts: AHashMap<u64, usize> = AHashMap::new();
    let mut unique_to_b_counts: AHashMap<u64, usize> = AHashMap::new();

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
    println!("Comparison complete.");


    // --- PASS 2: 并行根据唯一的哈希取回行文本 ---
    println!("Pass 2: Collecting unique lines...");
    let handle_collect_a = thread::spawn(move || {
        collect_unique_lines(&file_a_path, unique_to_a_counts)
    });

    let handle_collect_b = thread::spawn(move || {
        collect_unique_lines(&file_b_path, unique_to_b_counts)
    });

    let unique_to_a_vec = handle_collect_a.join().unwrap()?;
    let unique_to_b_vec = handle_collect_b.join().unwrap()?;
    app.emit("progress", ProgressPayload { percentage: 100.0, file: "B".to_string(), text: "Comparison Finished".to_string() }).unwrap();
    println!("Pass 2: Complete.");

    // --- 最后一步: 发送最终结果 ---
    println!("Emitting final results...");
    if let Err(e) = app.emit("diff", DiffPayload { unique_to_a: unique_to_a_vec, unique_to_b: unique_to_b_vec }) {
        eprintln!("Failed to emit diff results: {}", e);
    }
    println!("All done.");

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_comparison])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}