use rand::prelude::*;
use rand::thread_rng;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

// Generates a random alphanumeric string of a given length.
fn generate_random_line(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// Scenario 1: Creates a second file based on the first with some lines missing.
pub fn generate_files_with_missing_lines(
    base_path: &Path,
    modified_path: &Path,
    num_lines_to_generate: usize,
    num_missing_lines: usize,
) {
    let base_file = File::create(base_path).expect("Failed to create base file.");
    let modified_file = File::create(modified_path).expect("Failed to create modified file.");
    let mut base_writer = BufWriter::new(base_file);
    let mut modified_writer = BufWriter::new(modified_file);
    let mut rng = thread_rng();

    let mut missing_indices: HashSet<usize> = HashSet::new();
    while missing_indices.len() < num_missing_lines {
        missing_indices.insert(rng.gen_range(0..num_lines_to_generate));
    }

    for i in 0..num_lines_to_generate {
        let line = generate_random_line(99); // 99 chars + newline = 100 bytes per line
        writeln!(base_writer, "{}", &line).unwrap();

        if !missing_indices.contains(&i) {
            writeln!(modified_writer, "{}", &line).unwrap();
        }
    }
}

// Scenario 2: Creates a second file with some lines duplicated.
pub fn generate_files_with_duplicated_lines(
    base_path: &Path,
    modified_path: &Path,
    num_lines_to_generate: usize,
    num_duplicated_lines: usize,
) {
    let base_file = File::create(base_path).expect("Failed to create base file.");
    let modified_file = File::create(modified_path).expect("Failed to create modified file.");
    let mut base_writer = BufWriter::new(base_file);
    let mut modified_writer = BufWriter::new(modified_file);
    let mut rng = thread_rng();

    let mut duplicate_indices: HashSet<usize> = HashSet::new();
    while duplicate_indices.len() < num_duplicated_lines {
        duplicate_indices.insert(rng.gen_range(0..num_lines_to_generate));
    }

    for i in 0..num_lines_to_generate {
        let line = generate_random_line(99);
        writeln!(base_writer, "{}", &line).unwrap();
        writeln!(modified_writer, "{}", &line).unwrap();

        if duplicate_indices.contains(&i) {
            writeln!(modified_writer, "{}", &line).unwrap(); // Write the same line again
        }
    }
}

// Scenario 3: Creates a second file with some fields in some lines modified.
pub fn generate_files_with_modified_lines(
    base_path: &Path,
    modified_path: &Path,
    num_lines_to_generate: usize,
    num_modified_lines: usize,
) {
    let base_file = File::create(base_path).expect("Failed to create base file.");
    let modified_file = File::create(modified_path).expect("Failed to create modified file.");
    let mut base_writer = BufWriter::new(base_file);
    let mut modified_writer = BufWriter::new(modified_file);
    let mut rng = thread_rng();

    let mut modified_indices: HashSet<usize> = HashSet::new();
    while modified_indices.len() < num_modified_lines {
        modified_indices.insert(rng.gen_range(0..num_lines_to_generate));
    }

    for i in 0..num_lines_to_generate {
        let line = format!(
            "id_{},data_{},value_{}",
            generate_random_line(10),
            generate_random_line(50),
            generate_random_line(20)
        );
        writeln!(base_writer, "{}", &line).unwrap();

        if modified_indices.contains(&i) {
            let mut parts: Vec<&str> = line.split(',').collect();
            let modified_line_str = format!("value_MODIFIED");
            parts[2] = &modified_line_str;
            let modified_line = parts.join(",");
            writeln!(modified_writer, "{}", modified_line).unwrap();
        } else {
            writeln!(modified_writer, "{}", &line).unwrap();
        }
    }
}

// A comprehensive scenario combining missing, duplicated, and modified lines.
pub fn generate_files_with_comprehensive_diffs(
    base_path: &Path,
    modified_path: &Path,
    num_lines_to_generate: usize,
    num_diffs: usize,
) {
    let base_file = File::create(base_path).expect("Failed to create base file.");
    let modified_file = File::create(modified_path).expect("Failed to create modified file.");
    let mut base_writer = BufWriter::new(base_file);
    let mut modified_writer = BufWriter::new(modified_file);
    let mut rng = thread_rng();

    let mut diff_indices: HashSet<usize> = HashSet::new();
    while diff_indices.len() < num_diffs {
        diff_indices.insert(rng.gen_range(0..num_lines_to_generate));
    }

    let diffs_per_category = num_diffs / 3;
    let mut iter = diff_indices.into_iter();

    let mut missing_indices: HashSet<usize> = (0..diffs_per_category).map(|_| iter.next().unwrap()).collect();
    let mut duplicated_indices: HashSet<usize> = (0..diffs_per_category).map(|_| iter.next().unwrap()).collect();
    let modified_indices: HashSet<usize> = iter.collect();

    for i in 0..num_lines_to_generate {
        let line = format!(
            "id_{:09},data_{},value_{}",
            i,
            generate_random_line(50),
            generate_random_line(20)
        );
        writeln!(base_writer, "{}", &line).unwrap();

        if missing_indices.contains(&i) {
            // Don't write the line to the modified file
        } else if duplicated_indices.contains(&i) {
            writeln!(modified_writer, "{}", &line).unwrap();
            writeln!(modified_writer, "{}", &line).unwrap();
        } else if modified_indices.contains(&i) {
            let mut parts: Vec<&str> = line.split(',').collect();
            let original_value = parts[2];
            let modified_line_str = format!("{}_MODIFIED", original_value);
            parts[2] = &modified_line_str;
            let modified_line = parts.join(",");
            writeln!(modified_writer, "{}", modified_line).unwrap();
        } else {
            writeln!(modified_writer, "{}", &line).unwrap();
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const TEST_DIR: &str = "test_output";
    // To generate 1GB file, we need about 10 million lines of 100 bytes each.
    // 1_073_741_824 bytes / 100 bytes/line = 10,737,418 lines
    const NUM_LINES_FOR_1GB: usize = 10_737_418;
    const NUM_DIFFERENCES: usize = 30;

    #[test]
    #[ignore] // Ignored by default because it generates large files and takes time.
    fn test_generate_files_with_missing_lines() {
        fs::create_dir_all(TEST_DIR).unwrap();
        let base_path = Path::new(TEST_DIR).join("missing_base.txt");
        let modified_path = Path::new(TEST_DIR).join("missing_modified.txt");
        generate_files_with_missing_lines(
            &base_path,
            &modified_path,
            NUM_LINES_FOR_1GB,
            NUM_DIFFERENCES,
        );
    }

    #[test]
    #[ignore]
    fn test_generate_files_with_duplicated_lines() {
        fs::create_dir_all(TEST_DIR).unwrap();
        let base_path = Path::new(TEST_DIR).join("duplicated_base.txt");
        let modified_path = Path::new(TEST_DIR).join("duplicated_modified.txt");
        generate_files_with_duplicated_lines(
            &base_path,
            &modified_path,
            NUM_LINES_FOR_1GB,
            NUM_DIFFERENCES,
        );
    }

    #[test]
    #[ignore]
    fn test_generate_files_with_modified_lines() {
        fs::create_dir_all(TEST_DIR).unwrap();
        let base_path = Path::new(TEST_DIR).join("modified_base.txt");
        let modified_path = Path::new(TEST_DIR).join("modified_modified.txt");
        generate_files_with_modified_lines(
            &base_path,
            &modified_path,
            NUM_LINES_FOR_1GB,
            NUM_DIFFERENCES,
        );
    }

    #[test]
    #[ignore]
    fn test_generate_files_with_comprehensive_diffs() {
        fs::create_dir_all(TEST_DIR).unwrap();
        let base_path = Path::new(TEST_DIR).join("comprehensive_base.txt");
        let modified_path = Path::new(TEST_DIR).join("comprehensive_modified.txt");
        generate_files_with_comprehensive_diffs(
            &base_path,
            &modified_path,
            NUM_LINES_FOR_1GB,
            NUM_DIFFERENCES,
        );
    }
}