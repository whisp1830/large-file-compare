use crate::external::file_processing::{collect_unique_lines, partition_file, HashOffset, NUM_PARTITIONS};
use crate::payloads::{ComparisonFinishedPayload, ProgressPayload, StepDetailPayload};
use crate::CompareConfig;
use extsort::Sortable;
use gxhash::HashMap;
use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{BufReader, Error as IoError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use tauri::{AppHandle, Emitter};

fn read_partition_into_maps(
    partition_path: PathBuf,
) -> Result<(HashMap<u64, usize>, HashMap<u64, u64>), IoError> {
    let mut counts = HashMap::default();
    let mut first_offsets = HashMap::default();

    if !partition_path.exists() {
        return Ok((counts, first_offsets));
    }

    let file = File::open(partition_path)?;
    let mut reader = BufReader::new(file);

    while let Ok(item) = HashOffset::decode(&mut reader) {
        *counts.entry(item.0).or_insert(0) += 1;
        first_offsets.entry(item.0).or_insert(item.1);
    }

    Ok((counts, first_offsets))
}

pub fn run_comparison(
    app: AppHandle,
    file_a_path: String,
    file_b_path: String,
    compare_config: CompareConfig,
) -> Result<(), IoError> {
    let start_time = std::time::Instant::now();
    let temp_dir = std::env::temp_dir().join(format!("bcomp_{}", start_time.elapsed().as_nanos()));
    let temp_dir_a = temp_dir.join("a");
    let temp_dir_b = temp_dir.join("b");

    let app_a = app.clone();
    let path_a_clone = file_a_path.clone();
    let temp_dir_a_clone = temp_dir_a.clone();
    let config_a_clone = compare_config.clone();

    let app_b = app.clone();
    let path_b_clone = file_b_path.clone();
    let temp_dir_b_clone = temp_dir_b.clone();
    let config_b_clone = compare_config.clone();

    let (nl_path_a, nl_path_b) = if compare_config.use_single_thread {
        let path_a = partition_file(
            &app_a,
            &path_a_clone,
            &temp_dir_a_clone,
            "A",
            &compare_config,
        )?;
        let path_b = partition_file(
            &app_b,
            &path_b_clone,
            &temp_dir_b_clone,
            "B",
            &compare_config,
        )?;
        (path_a, path_b)
    } else {
        let handle_a_thread = thread::spawn(move || {
            partition_file(
                &app_a,
                &path_a_clone,
                &temp_dir_a_clone,
                "A",
                &config_a_clone,
            )
        });
        let handle_b_thread = thread::spawn(move || {
            partition_file(
                &app_b,
                &path_b_clone,
                &temp_dir_b_clone,
                "B",
                &config_b_clone,
            )
        });
        let path_a = handle_a_thread.join().unwrap()?;
        let path_b = handle_b_thread.join().unwrap()?;
        (path_a, path_b)
    };

    app.emit(
        "progress",
        ProgressPayload {
            percentage: 50.0,
            file: "A".to_string(),
            text: "Aggregating partitions...".to_string(),
        },
    )
    .unwrap();

    let now = std::time::Instant::now();
    let progress_counter = AtomicUsize::new(0);

    let (unique_to_a, unique_to_b): (Vec<_>, Vec<_>) = (0..NUM_PARTITIONS)
        .into_par_iter()
        .map(|i| {
            let part_a_path = temp_dir_a.join(format!("part_{}", i));
            let part_b_path = temp_dir_b.join(format!("part_{}", i));

            let (counts_a, offsets_a) = read_partition_into_maps(part_a_path).unwrap_or_default();
            let (counts_b, offsets_b) = read_partition_into_maps(part_b_path).unwrap_or_default();

            let mut partition_unique_a = Vec::new();
            let mut partition_unique_b = Vec::new();

            for (hash, &count_a) in &counts_a {
                let count_b = counts_b.get(hash).copied().unwrap_or(0);
                if compare_config.ignore_occurences && count_b > 0 {
                } else if count_a > count_b {
                    if let Some(&offset) = offsets_a.get(hash) {
                        partition_unique_a.push((offset, count_a - count_b));
                    }
                }
            }

            for (hash, &count_b) in &counts_b {
                let count_a = counts_a.get(hash).copied().unwrap_or(0);
                if compare_config.ignore_occurences && count_a > 0 {
                } else if count_b > count_a {
                    if let Some(&offset) = offsets_b.get(hash) {
                        partition_unique_b.push((offset, count_b - count_a));
                    }
                }
            }

            let processed_count = progress_counter.fetch_add(1, Ordering::Relaxed);
            let percentage = (processed_count as f64 / NUM_PARTITIONS as f64) * 50.0 + 50.0;
            app.emit(
                "progress",
                ProgressPayload {
                    percentage,
                    file: "B".to_string(),
                    text: "Aggregating partitions...".to_string(),
                },
            )
            .unwrap();

            (partition_unique_a, partition_unique_b)
        })
        .reduce(
            || (Vec::new(), Vec::new()),
            |mut a, b| {
                a.0.extend(b.0);
                a.1.extend(b.1);
                a
            },
        );

    let aggregation_ms = now.elapsed().as_millis();
    app.emit(
        "step_completed",
        StepDetailPayload {
            step: "Partition Aggregation".to_string(),
            duration_ms: aggregation_ms,
        },
    )
    .unwrap();

    let app_a_collect = app.clone();
    let config_for_a = compare_config.clone();
    let handle_collect_a = thread::spawn(move || {
        collect_unique_lines(
            &app_a_collect,
            &file_a_path,
            &unique_to_a,
            nl_path_a.as_ref(),
            &config_for_a,
            "A",
        )
    });

    let app_b_collect = app.clone();
    let config_for_b = compare_config.clone();
    let handle_collect_b = thread::spawn(move || {
        collect_unique_lines(
            &app_b_collect,
            &file_b_path,
            &unique_to_b,
            nl_path_b.as_ref(),
            &config_for_b,
            "B",
        )
    });

    handle_collect_a.join().unwrap()?;
    handle_collect_b.join().unwrap()?;

    fs::remove_dir_all(temp_dir)?;

    app.emit(
        "progress",
        ProgressPayload {
            percentage: 100.0,
            file: "B".to_string(),
            text: "Comparison Finished".to_string(),
        },
    )
    .unwrap();
    app.emit("comparison_finished", ComparisonFinishedPayload {})
        .unwrap();
    println!("All done in {}ms.", start_time.elapsed().as_millis());

    Ok(())
}
