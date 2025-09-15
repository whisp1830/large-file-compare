// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use tauri::AppHandle;
use crate::external::comparison;
use crate::internal::comparison_in_memory;

mod external {
    pub mod comparison;
    pub mod file_processing;
}

mod internal {
    pub mod comparison_in_memory;
    pub mod file_processing_in_memory;
}
mod payloads;

#[derive(Clone)]
struct CompareConfig {
    use_external_sort: bool,
    ignore_occurences: bool,
    use_single_thread: bool,
    ignore_line_number: bool,
    primary_key_regex: String,
}

#[tauri::command]
async fn start_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
    use_external_sort: bool,
    ignore_occurences: bool,
    use_single_thread: bool,
    ignore_line_number: bool,
    primary_key_regex: String,
) -> Result<(), String> {
    let compare_config = CompareConfig {use_external_sort, ignore_occurences, use_single_thread, ignore_line_number, primary_key_regex};
    thread::spawn(move || {
        if compare_config.use_external_sort {
            if let Err(e) = comparison::run_comparison(app, file_a_path, file_b_path, compare_config) {
                // Handle errors, maybe emit an event to the frontend
                eprintln!("Comparison failed: {}", e);
            }
        } else {
            if let Err(e) = comparison_in_memory::run_comparison(app, file_a_path, file_b_path, compare_config) {
                // Handle errors, maybe emit an event to the frontend
                eprintln!("Comparison failed: {}", e);
            }
        }

    });
    Ok(())
}

use std::fs;

#[tauri::command]
fn save_file(path: String, content: String) -> Result<(), String> {
    fs::write(path, content).map_err(|err| err.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_comparison, save_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}