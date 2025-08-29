use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::fs;

pub struct FileModifier {
    file_path: PathBuf,
}

impl FileModifier {
    pub fn new(file_path: &Path) -> Self {
        Self {
            file_path: file_path.to_path_buf(),
        }
    }

    /// Adds a new line after the specified line number.
    /// Line numbers are 1-based.
    pub fn add_line_after(&self, line_number: usize, new_line: &str) -> io::Result<()> {
        let temp_path = self.file_path.with_extension("tmp");
        let original_file = File::open(&self.file_path)?;
        let reader = BufReader::new(original_file);
        let mut temp_file = OpenOptions::new().write(true).create(true).truncate(true).open(&temp_path)?;

        for (current_line_num, line) in reader.lines().enumerate() {
            let line = line?;
            writeln!(temp_file, "{}", line)?;
            if (current_line_num + 1) == line_number {
                writeln!(temp_file, "{}", new_line)?;
            }
        }

        fs::rename(&temp_path, &self.file_path)?;
        Ok(())
    }

    /// Deletes the specified line number.
    /// Line numbers are 1-based.
    pub fn delete_line(&self, line_number: usize) -> io::Result<()> {
        let temp_path = self.file_path.with_extension("tmp");
        let original_file = File::open(&self.file_path)?;
        let reader = BufReader::new(original_file);
        let mut temp_file = OpenOptions::new().write(true).create(true).truncate(true).open(&temp_path)?;

        for (current_line_num, line) in reader.lines().enumerate() {
            if (current_line_num + 1) != line_number {
                let line = line?;
                writeln!(temp_file, "{}", line)?;
            }
        }

        fs::rename(&temp_path, &self.file_path)?;
        Ok(())
    }

    /// Replaces the content of the specified line number.
    /// Line numbers are 1-based.
    pub fn replace_line(&self, line_number: usize, new_content: &str) -> io::Result<()> {
        let temp_path = self.file_path.with_extension("tmp");
        let original_file = File::open(&self.file_path)?;
        let reader = BufReader::new(original_file);
        let mut temp_file = OpenOptions::new().write(true).create(true).truncate(true).open(&temp_path)?;

        for (current_line_num, line) in reader.lines().enumerate() {
            if (current_line_num + 1) == line_number {
                writeln!(temp_file, "{}", new_content)?;
            } else {
                let line = line?;
                writeln!(temp_file, "{}", line)?;
            }
        }

        fs::rename(&temp_path, &self.file_path)?;
        Ok(())
    }

}

fn main() {
    let path = PathBuf::from("F:\\testscript\\file1_original.txt");
    let modifier = FileModifier::new(&path);

    modifier.add_line_after(1, "line 2").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::prelude::*;

    // Helper functions to create and read test files
    fn create_test_file(path: &Path, content: &str) -> io::Result<()> {
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn read_test_file(path: &Path) -> io::Result<String> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    #[test]
    fn test_add_line() {
        let path = PathBuf::from("test_add.txt");
        create_test_file(&path, "line 1\nline 3").unwrap();
        let modifier = FileModifier::new(&path);
        
        modifier.add_line_after(1, "line 2").unwrap();
        
        let content = read_test_file(&path).unwrap();
        assert_eq!(content, "line 1\nline 2\nline 3\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_delete_line() {
        let path = PathBuf::from("test_delete.txt");
        create_test_file(&path, "line 1\nline 2\nline 3").unwrap();
        let modifier = FileModifier::new(&path);

        modifier.delete_line(2).unwrap();

        let content = read_test_file(&path).unwrap();
        assert_eq!(content, "line 1\nline 3\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_replace_line() {
        let path = PathBuf::from("test_replace.txt");
        create_test_file(&path, "line 1\nold line\nline 3").unwrap();
        let modifier = FileModifier::new(&path);

        modifier.replace_line(2, "new line").unwrap();

        let content = read_test_file(&path).unwrap();
        assert_eq!(content, "line 1\nnew line\nline 3\n");
        fs::remove_file(path).unwrap();
    }
}

