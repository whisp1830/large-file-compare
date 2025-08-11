use crate::exmemory::file_processing::{collect_unique_lines, create_sorted_hash_file, HashOffset};
use crate::payloads::{ComparisonFinishedPayload, ProgressPayload, StepDetailPayload};
use extsort::Sortable;
use std::fs::File;
use std::io::{BufReader, Error as IoError};
use std::thread;
use tauri::{AppHandle, Emitter};

pub fn run_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
) -> Result<(), IoError> {
    let start_time = std::time::Instant::now();
    let temp_dir = std::env::temp_dir();
    let sorted_a_path = temp_dir.join("sorted_a.bin");
    let sorted_b_path = temp_dir.join("sorted_b.bin");

    // --- Step 1: Create sorted hash files in parallel ---
    let app_a = app.clone();
    let path_a_clone = file_a_path.clone();
    let sorted_a_path_clone = sorted_a_path.clone();
    let handle_a = thread::spawn(move || {
        create_sorted_hash_file(&app_a, &path_a_clone, &sorted_a_path_clone, "A")
    });

    let app_b = app.clone();
    let path_b_clone = file_b_path.clone();
    let sorted_b_path_clone = sorted_b_path.clone();
    let handle_b = thread::spawn(move || {
        create_sorted_hash_file(&app_b, &path_b_clone, &sorted_b_path_clone, "B")
    });

    handle_a.join().unwrap()?;
    handle_b.join().unwrap()?;
    app.emit("progress", ProgressPayload { percentage: 50.0, file: "A".to_string(), text: "Comparing differences...".to_string() }).unwrap();

    // --- Step 2: Compare sorted files (Reduce phase) ---
    let now = std::time::Instant::now();
    let mut unique_to_a: Vec<(u64, usize)> = Vec::new();
    let mut unique_to_b: Vec<(u64, usize)> = Vec::new();

    let mut reader_a = BufReader::new(File::open(&sorted_a_path)?);
    let mut reader_b = BufReader::new(File::open(&sorted_b_path)?);

    let mut item_a = HashOffset::decode(&mut reader_a).ok();
    let mut item_b = HashOffset::decode(&mut reader_b).ok();

    while item_a.is_some() || item_b.is_some() {
        match (item_a, item_b) {
            (Some(a), Some(b)) => {
                if a.0 < b.0 {
                    // A is unique
                    let (count, next_a) = count_and_advance(&mut reader_a, a.0);
                    unique_to_a.push((a.1, count));
                    item_a = next_a;
                    item_b = Some(b);
                } else if a.0 > b.0 {
                    // B is unique
                    let (count, next_b) = count_and_advance(&mut reader_b, b.0);
                    unique_to_b.push((b.1, count));
                    item_b = next_b;
                    item_a = Some(a);
                } else {
                    // Hashes are equal, compare counts
                    let (count_a, next_a) = count_and_advance(&mut reader_a, a.0);
                    let (count_b, next_b) = count_and_advance(&mut reader_b, b.0);
                    if count_a > count_b {
                        unique_to_a.push((a.1, count_a - count_b));
                    } else if count_b > count_a {
                        unique_to_b.push((b.1, count_b - count_a));
                    }
                    item_a = next_a;
                    item_b = next_b;
                }
            }
            (Some(a), None) => {
                // A is unique
                let (count, next_a) = count_and_advance(&mut reader_a, a.0);
                unique_to_a.push((a.1, count));
                item_a = next_a;
            }
            (None, Some(b)) => {
                // B is unique
                let (count, next_b) = count_and_advance(&mut reader_b, b.0);
                unique_to_b.push((b.1, count));
                item_b = next_b;
            }
            (None, None) => break,
        }
    }

    let comparison_ms = now.elapsed().as_millis();
    app.emit("step_completed", StepDetailPayload { step: "Reduce & Compare".to_string(), duration_ms: comparison_ms }).unwrap();

    // --- Step 3: Collect unique lines ---
    let app_a_collect = app.clone();
    let handle_collect_a = thread::spawn(move || {
        collect_unique_lines(&app_a_collect, &file_a_path, &unique_to_a, "A")
    });

    let app_b_collect = app.clone();
    let handle_collect_b = thread::spawn(move || {
        collect_unique_lines(&app_b_collect, &file_b_path, &unique_to_b, "B")
    });

    handle_collect_a.join().unwrap()?;
    handle_collect_b.join().unwrap()?;

    // --- Finalize ---
    std::fs::remove_file(sorted_a_path)?;
    std::fs::remove_file(sorted_b_path)?;

    app.emit("progress", ProgressPayload { percentage: 100.0, file: "B".to_string(), text: "Comparison Finished".to_string() }).unwrap();
    app.emit("comparison_finished", ComparisonFinishedPayload {}).unwrap();
    println!("All done in {}ms.", start_time.elapsed().as_millis());

    Ok(())
}

fn count_and_advance(
    reader: &mut BufReader<File>,
    current_hash: u64,
) -> (usize, Option<HashOffset>) {
    let mut count = 1;
    loop {
        match HashOffset::decode(reader) {
            Ok(hash_offset) => {
                if hash_offset.0 == current_hash {
                    count += 1;
                } else {
                    return (count, Some(hash_offset));
                }
            }
            Err(_) => {
                return (count, None);
            }
        }
    }
}