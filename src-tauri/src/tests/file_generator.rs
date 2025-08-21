use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct FileGenerator {
    lines: Vec<String>,
}

impl FileGenerator {
    pub fn new(lines: Vec<String>) -> Self {
        Self { lines }
    }

    pub fn write_lines(&self, path: &Path, lines: &[String]) {
        let file = File::create(path).unwrap();
        let mut writer = BufWriter::new(file);
        for line in lines {
            writeln!(writer, "{}", line).unwrap();
        }
    }
}