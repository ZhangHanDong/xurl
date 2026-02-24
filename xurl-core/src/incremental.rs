use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Tracks a file position for incremental JSONL reading.
///
/// Each call to [`read_new_lines`] reads only the data appended since the
/// previous call, making it suitable for polling-based monitoring.
pub struct IncrementalReader {
    path: PathBuf,
    offset: u64,
}

impl IncrementalReader {
    /// Create a reader starting from the beginning of the file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
        }
    }

    /// Create a reader positioned at the current end of the file,
    /// so only newly appended data will be returned.
    pub fn from_end(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self { path, offset }
    }

    /// Current byte offset into the file.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The path being tracked.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read lines appended since the last call.
    ///
    /// Each line is parsed as JSON. Lines that are empty or fail to parse
    /// are silently skipped. Returns successfully parsed values.
    pub fn read_new_lines(&mut self) -> Vec<Value> {
        let file_len = match fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return Vec::new(),
        };

        if file_len <= self.offset {
            return Vec::new();
        }

        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let mut reader = BufReader::new(file);
        if self.offset > 0 {
            if reader.seek(SeekFrom::Start(self.offset)).is_err() {
                return Vec::new();
            }
        }

        let mut values = Vec::new();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(n) => {
                    self.offset += n as u64;
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                        values.push(val);
                    }
                }
                Err(_) => break,
            }
        }

        values
    }

    /// Check whether the file has grown since the last read.
    pub fn has_new_data(&self) -> bool {
        fs::metadata(&self.path)
            .map(|m| m.len() > self.offset)
            .unwrap_or(false)
    }

    /// Reset the reader to the beginning of the file.
    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn read_then_append_then_read() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("test.jsonl");
        fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").expect("write");

        let mut reader = IncrementalReader::new(&path);
        let batch1 = reader.read_new_lines();
        assert_eq!(batch1.len(), 2);
        assert_eq!(batch1[0]["a"], 1);
        assert_eq!(batch1[1]["b"], 2);

        // No new data
        let batch2 = reader.read_new_lines();
        assert!(batch2.is_empty());
        assert!(!reader.has_new_data());

        // Append more
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        writeln!(file, "{{\"c\":3}}").expect("append");
        drop(file);

        assert!(reader.has_new_data());
        let batch3 = reader.read_new_lines();
        assert_eq!(batch3.len(), 1);
        assert_eq!(batch3[0]["c"], 3);
    }

    #[test]
    fn from_end_skips_existing() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("test.jsonl");
        fs::write(&path, "{\"old\":true}\n").expect("write");

        let mut reader = IncrementalReader::from_end(&path);
        let batch = reader.read_new_lines();
        assert!(batch.is_empty());

        // Append new data
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        writeln!(file, "{{\"new\":true}}").expect("append");
        drop(file);

        let batch = reader.read_new_lines();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0]["new"], true);
    }

    #[test]
    fn invalid_json_lines_skipped() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("test.jsonl");
        fs::write(&path, "{\"ok\":1}\nnot json\n{\"ok\":2}\n").expect("write");

        let mut reader = IncrementalReader::new(&path);
        let batch = reader.read_new_lines();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0]["ok"], 1);
        assert_eq!(batch[1]["ok"], 2);
    }

    #[test]
    fn missing_file_returns_empty() {
        let mut reader = IncrementalReader::new("/nonexistent/path.jsonl");
        let batch = reader.read_new_lines();
        assert!(batch.is_empty());
        assert!(!reader.has_new_data());
    }

    #[test]
    fn reset_rereads_from_start() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("test.jsonl");
        fs::write(&path, "{\"x\":1}\n").expect("write");

        let mut reader = IncrementalReader::new(&path);
        let _ = reader.read_new_lines();
        assert!(!reader.has_new_data());

        reader.reset();
        let batch = reader.read_new_lines();
        assert_eq!(batch.len(), 1);
    }
}
