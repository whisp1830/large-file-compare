// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::thread;
use ahash::AHasher;
use std::hash::Hasher;
use memmap2::Mmap;
use std::io::Error as IoError;

use tauri::{AppHandle, Emitter};

const BUFFER_SIZE: usize = 1024 * 1024; // 1MB

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

// --- Core Hashing Logic ---

fn hash_line(line: &str) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(line.as_bytes());
    hasher.finish()
}

fn process_file_with_mmap(
    app: &AppHandle,
    file_path: &str,
    progress_file_id: &str,
) -> Result<HashMap<u64, (String, usize)>, IoError> {

    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    if file_size == 0 {
        return Ok(HashMap::new());
    }

    // SAFETY: 我们假设在程序读取期间文件不会被外部修改。
    // 对于只读操作这是个合理的假设。
    let mmap = unsafe { Mmap::map(&file)? };

    let estimated_lines = (file_size / 50).max(1024) as usize;
    let mut line_hashes: HashMap<u64, (String, usize)> = HashMap::with_capacity(estimated_lines);

    let mut bytes_processed: u64 = 0;
    let mut last_emitted_percentage: f64 = -1.0;

    // 直接在内存映射的字节切片上按换行符分割
    for line_bytes in mmap.split(|&b| b == b'\n') {
        // 更新处理进度
        bytes_processed += line_bytes.len() as u64 + 1; // +1 for the newline character

        // 跳过空行
        if line_bytes.is_empty() {
            continue;
        }

        // 处理行尾可能存在的 '\r'
        let line_bytes = if line_bytes.last() == Some(&b'\r') {
            &line_bytes[..line_bytes.len() - 1]
        } else {
            line_bytes
        };

        // 尝试将字节转换为 &str，如果文件不是有效的 UTF-8 可能会失败
        if let Ok(line_str) = std::str::from_utf8(line_bytes) {
            let hash = hash_line(line_str);
            line_hashes
                .entry(hash)
                .and_modify(|e| e.1 += 1)
                .or_insert_with(|| (line_str.to_string(), 1));
        } else {
            // 你可以在这里处理非 UTF-8 行，例如跳过或记录错误
        }

        // 发送进度更新 (逻辑保持不变)
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

// --- Tauri Command ---

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
    let handle_a = thread::spawn(move || {
        process_file_with_mmap(&app_a, &file_a_path, "A")
    });

    let app_b = app.clone();
    let handle_b = thread::spawn(move || {
        process_file_with_mmap(&app_b, &file_b_path, "B")
    });

    // --- Step 2: 等待线程完成并获取结果 ---
    // .unwrap() 会在线程 panic 时 panic，这里我们假设它会返回 Result
    let mut map_a = handle_a.join().unwrap()?;
    let map_b = handle_b.join().unwrap()?;

    // --- Step 3: 比对两个 HashMap ---
    let mut unique_to_b_vec: Vec<String> = Vec::new();

    for (hash_b, (line_b, count_b)) in map_b {
        if let Some(entry_a) = map_a.get_mut(&hash_b) {
            // 存在于两个文件中，计算公共数量并更新 A 的计数
            let common_count = std::cmp::min(entry_a.1, count_b);
            entry_a.1 -= common_count;
        } else {
            // 只存在于文件 B
            unique_to_b_vec.push(if count_b > 1 { format!("{} (x{})", line_b, count_b) } else { line_b });
        }
    }

    // map_a 中计数大于 0 的就是只存在于文件 A 的
    let unique_to_a_vec: Vec<String> = map_a
        .values()
        .filter(|(_, count)| *count > 0)
        .map(|(line, count)| if *count > 1 { format!("{} (x{})", line, *count) } else { line.clone() })
        .collect();

    // --- Step 4: 发送最终结果 ---
    if let Err(e) = app.emit("diff", DiffPayload { unique_to_a: unique_to_a_vec, unique_to_b: unique_to_b_vec }) {
        eprintln!("Failed to emit diff results: {}", e);
    }

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_comparison])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
