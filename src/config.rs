//! Configuration management for hyperV
//! 
//! Handles configuration directory setup, file paths, and persistent storage.

use crate::error::{HyperVError, Result};
use std::fs;
use std::path::PathBuf;

/// Configuration manager for hyperV
pub struct Config {
    /// Base configuration directory
    pub config_dir: PathBuf,
    /// Path to tasks configuration file
    pub tasks_file: PathBuf,
    /// Directory for log files
    pub logs_dir: PathBuf,
}

impl Config {
    /// Initialize configuration directories and paths
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or(HyperVError::Config("Could not find config directory".to_string()))?
            .join("hyperV");

        let tasks_file = config_dir.join("tasks.json");
        let logs_dir = config_dir.join("logs");

        // Create directories if they don't exist
        fs::create_dir_all(&config_dir)
            .map_err(|e| HyperVError::Io(e))?;
        fs::create_dir_all(&logs_dir)
            .map_err(|e| HyperVError::Io(e))?;

        Ok(Config {
            config_dir,
            tasks_file,
            logs_dir,
        })
    }

    /// Get log directory for a specific task
    pub fn task_log_dir(&self, task_id: &str) -> PathBuf {
        self.logs_dir.join(task_id)
    }

    /// Get stdout log path for a task
    pub fn stdout_log_path(&self, task_id: &str) -> PathBuf {
        self.task_log_dir(task_id).join("stdout.log")
    }

    /// Get stderr log path for a task
    pub fn stderr_log_path(&self, task_id: &str) -> PathBuf {
        self.task_log_dir(task_id).join("stderr.log")
    }

    /// Ensure task log directory exists
    pub fn ensure_task_log_dir(&self, task_id: &str) -> Result<()> {
        let dir = self.task_log_dir(task_id);
        fs::create_dir_all(&dir)
            .map_err(|e| HyperVError::Io(e))?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new().expect("Failed to initialize configuration")
    }
}
