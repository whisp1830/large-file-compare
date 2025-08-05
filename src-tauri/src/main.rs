// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use tauri::AppHandle;

mod comparison;
mod file_processing;
mod payloads;

#[tauri::command]
async fn start_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
) -> Result<(), String> {
    thread::spawn(move || {
        if let Err(e) = comparison::run_comparison(app, file_a_path, file_b_path) {
            // Handle errors, maybe emit an event to the frontend
            eprintln!("Comparison failed: {}", e);
        }
    });
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_comparison])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}