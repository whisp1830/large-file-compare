// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Error as IoError;
use std::thread;
use ahash::AHasher;
use std::hash::Hasher;
use memmap2::Mmap;

use tauri::{AppHandle, Emitter};


// --- Data Structures for Frontend Communication ---

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    percentage: f64,
    file: String,
}

#[derive(Clone, serde::Serialize)]
struct DiffPayload {
    unique_to_a: Vec<String>,
    unique_to_b: Vec<String>,
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

        // 进度报告逻辑保持不变
        let percentage = (bytes_processed as f64 / file_size as f64) * 100.0;
        if percentage - last_emitted_percentage >= 3.0 || percentage >= 99.9 {
            if let Err(e) = app.emit("progress", ProgressPayload { percentage, file: progress_file_id.to_string() }) {
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
    mut unique_hashes: HashMap<u64, usize>,
) -> Result<Vec<String>, IoError> {
    if unique_hashes.is_empty() {
        return Ok(Vec::new());
    }

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(Vec::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };
    let mut results: Vec<String> = Vec::with_capacity(unique_hashes.len());

    for line_bytes in mmap.split(|&b| b == b'\n') {
        // 优化：如果已经找到了所有需要的行，就提前退出
        if unique_hashes.is_empty() {
            break;
        }

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

            // 使用 remove 来检查并获取计数，这样可以确保每行只被添加一次
            if let Some(count) = unique_hashes.remove(&hash) {
                let display_line = if count > 1 {
                    format!("{} (x{})", line_str, count)
                } else {
                    line_str.to_string()
                };
                results.push(display_line);
            }
        }
    }

    Ok(results)
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
    let mut map_a_counts = handle_a.join().unwrap()?;
    let mut map_b_counts = handle_b.join().unwrap()?;
    println!("Pass 1: Complete.");


    // --- 中间步骤: 比较哈希计数，找出独有的哈希 ---
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