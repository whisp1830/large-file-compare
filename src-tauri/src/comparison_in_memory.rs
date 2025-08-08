use crate::file_processing_in_memory::{collect_unique_lines_with_index, generate_hash_counts_and_index};
use crate::payloads::{ComparisonFinishedPayload, ProgressPayload, StepDetailPayload};
use gxhash::{HashMap, HashMapExt};
use std::thread;
use tauri::{AppHandle, Emitter};

pub fn run_comparison(
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