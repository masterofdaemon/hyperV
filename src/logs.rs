//! Log management for hyperV
//! 
//! Handles log file rotation, reading, and real-time following functionality.

use crate::constants::{MAX_LOG_SIZE, LOG_FOLLOW_INTERVAL};
use crate::error::{HyperVError, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
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

        let metadata = fs::metadata(log_path)
            .map_err(HyperVError::Io)?;

        if metadata.len() > MAX_LOG_SIZE {
            let backup_path = log_path.with_extension("log.old");
            
            // Remove old backup if it exists
            if backup_path.exists() {
                fs::remove_file(&backup_path)
                    .map_err(HyperVError::Io)?;
            }
            
            // Move current log to backup
            fs::rename(log_path, &backup_path)
                .map_err(HyperVError::Io)?;
            
            println!("📦 Rotated log file: {} -> {}", 
                log_path.display(), backup_path.display());
        }

        Ok(())
    }

    /// Read the last N lines from a log file
    pub fn read_log_lines(log_path: &Path, lines: usize) -> Result<Vec<String>> {
        if !log_path.exists() {
            return Ok(vec!["Log file not found or empty".to_string()]);
        }

        let file = File::open(log_path)
            .map_err(HyperVError::Io)?;
        
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader
            .lines()
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(HyperVError::Io)?;

        let start_index = if all_lines.len() > lines {
            all_lines.len() - lines
        } else {
            0
        };

        Ok(all_lines[start_index..].to_vec())
    }

    /// Show logs for a task
    pub fn show_logs(
        stdout_path: &Path,
        stderr_path: &Path,
        log_type: LogType,
        lines: usize,
        follow: bool,
    ) -> Result<()> {
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

    /// Show logs from a single file
    fn show_single_log(log_path: &Path, log_name: &str, lines: usize, follow: bool) -> Result<()> {
        println!("=== {} ===", log_name);
        
        let log_lines = Self::read_log_lines(log_path, lines)?;
        for line in log_lines {
            println!("{}", line);
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

        let mut file = File::open(log_path)
            .map_err(HyperVError::Io)?;
        
        // Seek to end of file
        file.seek(SeekFrom::End(0))
            .map_err(HyperVError::Io)?;
        
        let mut reader = BufReader::new(file);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // No new data, sleep and try again
                    thread::sleep(LOG_FOLLOW_INTERVAL);
                    continue;
                }
                Ok(_) => {
                    print!("{}", line);
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
    fn follow_both_logs(stdout_path: &Path, stderr_path: &Path) -> Result<()> {
        // This is a simplified implementation
        // In a production system, you might want to use async I/O or threads
        // to properly interleave stdout and stderr output
        
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
                    _ => {}
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
                    _ => {}
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

        let metadata = fs::metadata(log_path)
            .map_err(HyperVError::Io)?;
        
        let file = File::open(log_path)
            .map_err(HyperVError::Io)?;
        
        let reader = BufReader::new(file);
        let line_count = reader.lines().count();

        Ok(LogInfo {
            exists: true,
            size: metadata.len(),
            line_count,
        })
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
