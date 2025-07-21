// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::thread;
use ahash::AHasher;
use std::hash::Hasher;

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

// --- Core Hashing Logic ---

fn hash_line(line: &str) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(line.as_bytes());
    hasher.finish()
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
    // --- Step 1: Read File A and build the hash map ---
    let file_a = File::open(&file_a_path)?;
    let file_a_metadata = file_a.metadata()?;
    let file_a_size = file_a_metadata.len();
    let mut reader_a = BufReader::new(file_a);
    
    let mut line_hashes: HashMap<u64, (String, usize)> = HashMap::new();
    let mut bytes_read_a: u64 = 0;
    let mut line = String::new();
    let mut last_emitted_percentage_a: f64 = -1.0;

    while reader_a.read_line(&mut line)? > 0 {
        bytes_read_a += line.len() as u64;
        let trimmed_line = line.trim_end().to_string();
        let hash = hash_line(&trimmed_line);
        line_hashes.entry(hash).and_modify(|e| e.1 += 1).or_insert((trimmed_line, 1));
        line.clear();

        // Emit progress
        let percentage = (bytes_read_a as f64 / file_a_size as f64) * 100.0;
        if percentage - last_emitted_percentage_a >= 3.0 || percentage >= 99.9 {
            if let Err(e) = app.emit("progress", ProgressPayload { percentage, file: "A".to_string() }) {
                eprintln!("Failed to emit progress for File A: {}", e);
            }
            last_emitted_percentage_a = percentage;
        }
    }

    // --- Step 2: Read File B and compare ---
    let file_b = File::open(&file_b_path)?;
    let file_b_metadata = file_b.metadata()?;
    let file_b_size = file_b_metadata.len();
    let mut reader_b = BufReader::new(file_b);
    
    let mut unique_to_b: Vec<String> = Vec::new();
    let mut bytes_read_b: u64 = 0;
    let mut last_emitted_percentage_b: f64 = -1.0;
    line.clear();

    while reader_b.read_line(&mut line)? > 0 {
        bytes_read_b += line.len() as u64;
        let trimmed_line = line.trim_end().to_string();
        let hash = hash_line(&trimmed_line);

        if let Some(entry) = line_hashes.get_mut(&hash) {
            if entry.1 > 1 {
                entry.1 -= 1;
            } else {
                line_hashes.remove(&hash);
            }
        } else {
            unique_to_b.push(trimmed_line);
        }
        line.clear();

        // Emit progress
        let percentage = (bytes_read_b as f64 / file_b_size as f64) * 100.0;
        if percentage - last_emitted_percentage_b >= 3.0 || percentage >= 99.9 {
            if let Err(e) = app.emit("progress", ProgressPayload { percentage, file: "B".to_string() }) {
                eprintln!("Failed to emit progress for File B: {}", e);
            }
            last_emitted_percentage_b = percentage;
        }
    }

    // --- Step 3: Finalize and emit results ---
    let unique_to_a: Vec<String> = line_hashes.values().map(|(l, c)| format!("{} (x{})", l, c)).collect();

    if let Err(e) = app.emit("diff", DiffPayload { unique_to_a, unique_to_b }) {
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
