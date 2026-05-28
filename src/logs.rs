//! Log management for hyperV
//!
//! Handles log file rotation, reading, and real-time following functionality.

use crate::constants::{LOG_FOLLOW_INTERVAL, MAX_LOG_ARCHIVES, MAX_LOG_SIZE};
use crate::error::{HyperVError, Result};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;

/// Types of logs that can be viewed
#[derive(Debug, Clone, PartialEq)]
pub enum LogType {
    Stdout,
    Stderr,
    Both,
}

impl std::str::FromStr for LogType {
    type Err = HyperVError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "stdout" => Ok(LogType::Stdout),
            "stderr" => Ok(LogType::Stderr),
            "both" => Ok(LogType::Both),
            _ => Err(HyperVError::InvalidLogType(s.to_string())),
        }
    }
}

/// Log manager for handling log files
pub struct LogManager;

impl LogManager {
    /// Rotate a log file if it exceeds the maximum size
    pub fn rotate_log_if_needed(log_path: &Path) -> Result<()> {
        if !log_path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(log_path).map_err(HyperVError::Io)?;

        if metadata.len() > MAX_LOG_SIZE {
            Self::rotate_archives(log_path)?;
            let archive_path = Self::archive_path(log_path, 1)?;
            Self::compress_log_to_archive(log_path, &archive_path)?;
            fs::remove_file(log_path).map_err(HyperVError::Io)?;

            println!(
                "📦 Rotated log file: {} -> {}",
                log_path.display(),
                archive_path.display()
            );
        }

        Ok(())
    }

    fn rotate_archives(log_path: &Path) -> Result<()> {
        let oldest_archive = Self::archive_path(log_path, MAX_LOG_ARCHIVES)?;
        if oldest_archive.exists() {
            fs::remove_file(&oldest_archive).map_err(HyperVError::Io)?;
        }

        for archive_index in (1..MAX_LOG_ARCHIVES).rev() {
            let source = Self::archive_path(log_path, archive_index)?;
            if source.exists() {
                let destination = Self::archive_path(log_path, archive_index + 1)?;
                fs::rename(source, destination).map_err(HyperVError::Io)?;
            }
        }

        Ok(())
    }

    fn archive_path(log_path: &Path, archive_index: usize) -> Result<PathBuf> {
        let file_name = log_path
            .file_name()
            .ok_or_else(|| HyperVError::LogError("Log path has no file name".to_string()))?
            .to_string_lossy();

        Ok(log_path.with_file_name(format!("{file_name}.{archive_index}.gz")))
    }

    fn compress_log_to_archive(log_path: &Path, archive_path: &Path) -> Result<()> {
        let mut input = File::open(log_path).map_err(HyperVError::Io)?;
        let temp_archive = archive_path.with_extension("gz.tmp");
        let output = File::create(&temp_archive).map_err(HyperVError::Io)?;
        let mut encoder = GzEncoder::new(output, Compression::default());

        std::io::copy(&mut input, &mut encoder).map_err(HyperVError::Io)?;
        encoder.finish().map_err(HyperVError::Io)?;
        fs::rename(temp_archive, archive_path).map_err(HyperVError::Io)?;

        Ok(())
    }

    /// Read the last N lines from a log file
    pub fn read_log_lines(log_path: &Path, lines: usize) -> Result<Vec<String>> {
        if !log_path.exists() {
            return Ok(vec!["Log file not found or empty".to_string()]);
        }

        let file = File::open(log_path).map_err(HyperVError::Io)?;
        let file_size = file.metadata().map_err(HyperVError::Io)?.len();
        let mut reader = BufReader::new(file);

        if file_size == 0 {
            return Ok(Vec::new());
        }

        let mut result_lines = Vec::new();
        let mut buffer = Vec::new();
        let mut current_pos = file_size;

        // Read the file from the end in chunks
        while current_pos > 0 && result_lines.len() < lines {
            let chunk_size = std::cmp::min(current_pos, 4096);
            current_pos -= chunk_size;
            reader
                .seek(SeekFrom::Start(current_pos))
                .map_err(HyperVError::Io)?;
            let mut chunk = vec![0; chunk_size as usize];
            reader.read_exact(&mut chunk).map_err(HyperVError::Io)?;

            // Prepend the chunk to our buffer
            buffer.splice(0..0, chunk.iter().cloned());

            // Process the buffer to find lines
            while let Some(newline_pos) = buffer.iter().rposition(|&b| b == b'\n') {
                let line_bytes = buffer.split_off(newline_pos + 1);
                if !line_bytes.is_empty() {
                    result_lines.push(String::from_utf8_lossy(&line_bytes).trim_end().to_string());
                    if result_lines.len() >= lines {
                        break;
                    }
                }
                buffer.pop(); // Remove the newline character
            }
        }

        // Add the remaining buffer as the first line
        if !buffer.is_empty() && result_lines.len() < lines {
            result_lines.push(String::from_utf8_lossy(&buffer).trim_end().to_string());
        }

        result_lines.reverse();
        Ok(result_lines)
    }

    /// Show logs for a task
    pub fn show_logs(
        stdout_path: &Path,
        stderr_path: &Path,
        log_type: LogType,
        lines: usize,
        follow: bool,
        summary: bool,
    ) -> Result<()> {
        if summary {
            let summary = Self::summarize_logs(stdout_path, stderr_path, log_type)?;
            print!("{}", summary.format());
            return Ok(());
        }

        match log_type {
            LogType::Stdout => {
                Self::show_single_log(stdout_path, "STDOUT", lines, follow)?;
            }
            LogType::Stderr => {
                Self::show_single_log(stderr_path, "STDERR", lines, follow)?;
            }
            LogType::Both => {
                println!("=== STDOUT ===");
                let stdout_lines = Self::read_log_lines(stdout_path, lines / 2)?;
                for line in stdout_lines {
                    println!("{}", line);
                }

                println!("\n=== STDERR ===");
                let stderr_lines = Self::read_log_lines(stderr_path, lines / 2)?;
                for line in stderr_lines {
                    println!("{}", line);
                }

                if follow {
                    println!("\n=== Following logs (Ctrl+C to stop) ===");
                    Self::follow_both_logs(stdout_path, stderr_path)?;
                }
            }
        }

        Ok(())
    }

    /// Summarize selected logs without dumping full log content.
    pub fn summarize_logs(
        stdout_path: &Path,
        stderr_path: &Path,
        log_type: LogType,
    ) -> Result<LogSummary> {
        let mut summary = LogSummary::default();

        match log_type {
            LogType::Stdout => Self::summarize_log_family(stdout_path, "STDOUT", &mut summary)?,
            LogType::Stderr => Self::summarize_log_family(stderr_path, "STDERR", &mut summary)?,
            LogType::Both => {
                Self::summarize_log_family(stdout_path, "STDOUT", &mut summary)?;
                Self::summarize_log_family(stderr_path, "STDERR", &mut summary)?;
            }
        }

        summary.finalize();
        Ok(summary)
    }

    fn summarize_log_family(
        log_path: &Path,
        log_name: &str,
        summary: &mut LogSummary,
    ) -> Result<()> {
        let before_lines = summary.total_lines;
        let before_bytes = summary.total_bytes;

        if log_path.exists() {
            let metadata = fs::metadata(log_path).map_err(HyperVError::Io)?;
            summary.total_bytes += metadata.len();
            let file = File::open(log_path).map_err(HyperVError::Io)?;
            Self::summarize_reader(BufReader::new(file), log_name, false, summary)?;
        }

        for archive_index in 1..=MAX_LOG_ARCHIVES {
            let archive_path = Self::archive_path(log_path, archive_index)?;
            if !archive_path.exists() {
                continue;
            }

            let metadata = fs::metadata(&archive_path).map_err(HyperVError::Io)?;
            summary.total_bytes += metadata.len();
            summary.archive_count += 1;

            let file = File::open(&archive_path).map_err(HyperVError::Io)?;
            let decoder = GzDecoder::new(file);
            Self::summarize_reader(BufReader::new(decoder), log_name, true, summary)?;
        }

        summary.files.push(LogFileSummary {
            name: log_name.to_string(),
            path: log_path.to_string_lossy().to_string(),
            exists: log_path.exists(),
            lines: summary.total_lines - before_lines,
            bytes: summary.total_bytes - before_bytes,
        });

        Ok(())
    }

    fn summarize_reader<R: BufRead>(
        reader: R,
        log_name: &str,
        archived: bool,
        summary: &mut LogSummary,
    ) -> Result<()> {
        for line_result in reader.lines() {
            let line = line_result.map_err(HyperVError::Io)?;
            summary.total_lines += 1;

            let event = LogEvent::from_line(log_name, archived, &line);
            let message = event.message.clone();
            *summary.message_counts.entry(message).or_insert(0) += 1;

            match event.level {
                LogLevel::Error => {
                    summary.error_count += 1;
                    summary.recent_events.push(event);
                }
                LogLevel::Warn => {
                    summary.warning_count += 1;
                    summary.recent_events.push(event);
                }
                LogLevel::Info => {
                    summary.info_count += 1;
                }
                LogLevel::Other => {}
            }
        }

        Ok(())
    }

    /// Show logs from a single file
    fn show_single_log(log_path: &Path, log_name: &str, lines: usize, follow: bool) -> Result<()> {
        println!("=== {} ===", log_name);

        // Always show the most recent content
        if log_path.exists() {
            let log_lines = Self::read_log_lines(log_path, lines)?;
            for line in log_lines {
                println!("{}", line);
            }
        } else {
            println!(
                "No {} logs yet. Start the service to generate logs.",
                log_name
            );
            println!("Path: {}", log_path.display());
            return Ok(());
        }

        if follow {
            println!("\n=== Following {} (Ctrl+C to stop) ===", log_name);
            Self::follow_single_log(log_path)?;
        }

        Ok(())
    }

    /// Follow a single log file in real-time
    fn follow_single_log(log_path: &Path) -> Result<()> {
        if !log_path.exists() {
            println!("Log file not found: {}", log_path.display());
            return Ok(());
        }

        let mut file = File::open(log_path).map_err(HyperVError::Io)?;

        // Seek to end of file
        file.seek(SeekFrom::End(0)).map_err(HyperVError::Io)?;

        let mut reader = BufReader::new(file);
        let mut line = String::new();

        println!(
            "📖 Following log file: {} (Press Ctrl+C to stop)",
            log_path.display()
        );

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // No new data, sleep and try again
                    thread::sleep(LOG_FOLLOW_INTERVAL);

                    // Check if file was rotated or recreated
                    if let Ok(new_file) = File::open(log_path) {
                        let new_metadata = new_file.metadata().map_err(HyperVError::Io)?;
                        let current_metadata =
                            reader.get_ref().metadata().map_err(HyperVError::Io)?;

                        // If file size decreased or inode changed, file was rotated
                        if new_metadata.len() < current_metadata.len() {
                            println!("🔄 Log file rotated, reopening...");
                            file = new_file;
                            reader = BufReader::new(file);
                        }
                    }
                    continue;
                }
                Ok(_) => {
                    // Print with timestamp
                    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                    print!("[{}] {}", timestamp, line);
                }
                Err(e) => {
                    eprintln!("Error reading log: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Follow both stdout and stderr logs in real-time
    /// Follow both stdout and stderr logs in real-time
    fn follow_both_logs(stdout_path: &Path, stderr_path: &Path) -> Result<()> {
        let mut stdout_file = if stdout_path.exists() {
            let mut f = File::open(stdout_path).map_err(HyperVError::Io)?;
            f.seek(SeekFrom::End(0)).map_err(HyperVError::Io)?;
            Some(BufReader::new(f))
        } else {
            None
        };

        let mut stderr_file = if stderr_path.exists() {
            let mut f = File::open(stderr_path).map_err(HyperVError::Io)?;
            f.seek(SeekFrom::End(0)).map_err(HyperVError::Io)?;
            Some(BufReader::new(f))
        } else {
            None
        };

        let mut stdout_line = String::new();
        let mut stderr_line = String::new();

        println!("📖 Following logs (Press Ctrl+C to stop)");
        println!("OUT: {}", stdout_path.display());
        println!("ERR: {}", stderr_path.display());

        loop {
            let mut has_output = false;

            // Check stdout
            if let Some(ref mut reader) = stdout_file {
                stdout_line.clear();
                match reader.read_line(&mut stdout_line) {
                    Ok(n) if n > 0 => {
                        print!("[OUT] {}", stdout_line);
                        has_output = true;
                    }
                    Ok(_) => {
                        // Check rotation
                        if let Ok(new_file) = File::open(stdout_path)
                            && let Ok(new_meta) = new_file.metadata()
                            && let Ok(curr_meta) = reader.get_ref().metadata()
                            && new_meta.len() < curr_meta.len()
                        {
                            println!("🔄 Stdout log rotated, reopening...");
                            *reader = BufReader::new(new_file);
                        }
                    }
                    Err(_) => {}
                }
            } else if stdout_path.exists() {
                // File appeared
                if let Ok(f) = File::open(stdout_path) {
                    // Start from beginning or end? Tail usually starts from end if following,
                    // but if it just appeared we might want start. Let's start from beginning.
                    stdout_file = Some(BufReader::new(f));
                }
            }

            // Check stderr
            if let Some(ref mut reader) = stderr_file {
                stderr_line.clear();
                match reader.read_line(&mut stderr_line) {
                    Ok(n) if n > 0 => {
                        print!("[ERR] {}", stderr_line);
                        has_output = true;
                    }
                    Ok(_) => {
                        // Check rotation
                        if let Ok(new_file) = File::open(stderr_path)
                            && let Ok(new_meta) = new_file.metadata()
                            && let Ok(curr_meta) = reader.get_ref().metadata()
                            && new_meta.len() < curr_meta.len()
                        {
                            println!("🔄 Stderr log rotated, reopening...");
                            *reader = BufReader::new(new_file);
                        }
                    }
                    Err(_) => {}
                }
            } else if stderr_path.exists() {
                // File appeared
                if let Ok(f) = File::open(stderr_path) {
                    stderr_file = Some(BufReader::new(f));
                }
            }

            if !has_output {
                thread::sleep(LOG_FOLLOW_INTERVAL);
            }
        }
    }

    /// Get log file information
    pub fn get_log_info(log_path: &Path) -> Result<LogInfo> {
        if !log_path.exists() {
            return Ok(LogInfo {
                exists: false,
                size: 0,
                line_count: 0,
            });
        }

        let metadata = fs::metadata(log_path).map_err(HyperVError::Io)?;

        let file = File::open(log_path).map_err(HyperVError::Io)?;

        let reader = BufReader::new(file);
        let line_count = reader.lines().count();

        Ok(LogInfo {
            exists: true,
            size: metadata.len(),
            line_count,
        })
    }
}

/// Compact diagnostic summary for one `hyperV logs --summary` invocation.
#[derive(Debug, Default)]
pub struct LogSummary {
    pub total_lines: usize,
    pub total_bytes: u64,
    pub archive_count: usize,
    pub info_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub files: Vec<LogFileSummary>,
    top_messages: Vec<(usize, String)>,
    recent_events: Vec<LogEvent>,
    message_counts: HashMap<String, usize>,
}

impl LogSummary {
    fn finalize(&mut self) {
        let mut top_messages: Vec<(usize, String)> = self
            .message_counts
            .iter()
            .map(|(message, count)| (*count, message.clone()))
            .collect();
        top_messages.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        top_messages.truncate(8);
        self.top_messages = top_messages;

        let recent_start = self.recent_events.len().saturating_sub(10);
        self.recent_events = self.recent_events.split_off(recent_start);
    }

    pub fn format(&self) -> String {
        let mut output = String::new();

        output.push_str("=== LOG SUMMARY ===\n");
        output.push_str(&format!("Total lines: {}\n", self.total_lines));
        output.push_str(&format!("Total size: {}\n", format_bytes(self.total_bytes)));
        output.push_str(&format!("Archives scanned: {}\n", self.archive_count));
        output.push_str(&format!(
            "Levels: error={} warn={} info={} other={}\n",
            self.error_count,
            self.warning_count,
            self.info_count,
            self.total_lines
                .saturating_sub(self.error_count + self.warning_count + self.info_count)
        ));

        output.push_str("\nFiles:\n");
        for file in &self.files {
            let state = if file.exists { "present" } else { "missing" };
            output.push_str(&format!(
                "- {}: {} lines, {}, {} ({})\n",
                file.name,
                file.lines,
                format_bytes(file.bytes),
                state,
                file.path
            ));
        }

        output.push_str("\nTop repeated messages:\n");
        if self.top_messages.is_empty() {
            output.push_str("- none\n");
        } else {
            for (count, message) in &self.top_messages {
                output.push_str(&format!("- {}x {}\n", count, message));
            }
        }

        output.push_str("\nRecent warnings/errors:\n");
        if self.recent_events.is_empty() {
            output.push_str("- none\n");
        } else {
            for event in &self.recent_events {
                let archive_marker = if event.archived { " archived" } else { "" };
                output.push_str(&format!(
                    "- [{}{}] {} {}\n",
                    event.source, archive_marker, event.level, event.message
                ));
            }
        }

        output
    }
}

#[derive(Debug)]
pub struct LogFileSummary {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub lines: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Other,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Other => write!(f, "OTHER"),
        }
    }
}

#[derive(Debug, Clone)]
struct LogEvent {
    source: String,
    archived: bool,
    level: LogLevel,
    message: String,
}

impl LogEvent {
    fn from_line(source: &str, archived: bool, line: &str) -> Self {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            let level = value
                .get("level")
                .and_then(Value::as_str)
                .map(parse_level)
                .unwrap_or(LogLevel::Other);
            let message = value
                .get("msg")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or(line);

            return Self {
                source: source.to_string(),
                archived,
                level,
                message: sanitize_message(message),
            };
        }

        Self {
            source: source.to_string(),
            archived,
            level: parse_level(line),
            message: sanitize_message(line),
        }
    }
}

fn parse_level(input: &str) -> LogLevel {
    match input.to_ascii_uppercase().as_str() {
        "ERROR" => LogLevel::Error,
        "WARN" | "WARNING" => LogLevel::Warn,
        "INFO" => LogLevel::Info,
        value if value.contains("ERROR") || value.contains("PANIC") || value.contains("FAIL") => {
            LogLevel::Error
        }
        value if value.contains("WARN") => LogLevel::Warn,
        value if value.contains("INFO") => LogLevel::Info,
        _ => LogLevel::Other,
    }
}

fn sanitize_message(message: &str) -> String {
    let redacted = redact_sensitive_values(message);
    if redacted.chars().count() > 300 {
        format!("{}...", redacted.chars().take(300).collect::<String>())
    } else {
        redacted
    }
}

const SENSITIVE_REDACTION_KEYS: [&str; 11] = [
    "secret_token",
    "access_token",
    "auth_token",
    "api_token",
    "password",
    "api_key",
    "apikey",
    "passwd",
    "secret",
    "token",
    "key",
];

fn redact_sensitive_values(input: &str) -> String {
    let mut output = input.to_string();
    for key in SENSITIVE_REDACTION_KEYS {
        output = redact_key_assignments(&output, key);
    }
    output
}

fn redact_key_assignments(input: &str, key: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let lower = input.to_ascii_lowercase();
    let mut cursor = 0;

    while let Some(relative_pos) = lower[cursor..].find(key) {
        let key_start = cursor + relative_pos;
        let key_end = key_start + key.len();
        if !is_redaction_key_boundary(input, key_start) {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        }
        let Some(separator) = input[key_end..].chars().next() else {
            break;
        };

        if separator != '=' && separator != ':' {
            output.push_str(&input[cursor..key_end]);
            cursor = key_end;
            continue;
        }

        let value_start = key_end + separator.len_utf8();
        output.push_str(&input[cursor..value_start]);
        output.push_str("[REDACTED]");

        let mut value_end = value_start;
        for (offset, ch) in input[value_start..].char_indices() {
            if ch.is_whitespace() || matches!(ch, ',' | ';' | '&' | '"' | '\'' | '}') {
                break;
            }
            value_end = value_start + offset + ch.len_utf8();
        }
        cursor = value_end;
    }

    output.push_str(&input[cursor..]);
    output
}

fn is_redaction_key_boundary(input: &str, key_start: usize) -> bool {
    if key_start == 0 {
        return true;
    }

    input[..key_start]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_longer_sensitive_keys_before_prefix_keys() {
        let sanitized = sanitize_message("secret_token=abc123 secret=def456 monkey=banana");

        assert!(sanitized.contains("secret_token=[REDACTED]"));
        assert!(sanitized.contains("secret=[REDACTED]"));
        assert!(sanitized.contains("monkey=banana"));
        assert!(!sanitized.contains("abc123"));
        assert!(!sanitized.contains("def456"));
    }

    #[test]
    fn sensitive_redaction_keys_are_longest_first() {
        assert!(
            SENSITIVE_REDACTION_KEYS
                .windows(2)
                .all(|keys| keys[0].len() >= keys[1].len())
        );
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Information about a log file
#[derive(Debug)]
pub struct LogInfo {
    pub exists: bool,
    pub size: u64,
    pub line_count: usize,
}

impl LogInfo {
    /// Format the log size in human-readable format
    pub fn format_size(&self) -> String {
        if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        }
    }
}
