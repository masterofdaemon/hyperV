//! hyperV Service Manager
//! 
//! A command-line service manager for running and managing binary files
//! on Linux and macOS with advanced process management, logging, and monitoring.

pub mod cli;
pub mod config;
pub mod error;
pub mod logs;
pub mod manager;
pub mod process;
pub mod task;

pub use error::{HyperVError, Result};
pub use manager::TaskManager;
pub use task::{Task, TaskStatus};

/// Application constants
pub mod constants {
    use std::time::Duration;

    /// Maximum log file size before rotation (10MB)
    pub const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;
    
    /// Maximum number of automatic restart attempts
    pub const MAX_RESTART_ATTEMPTS: u32 = 5;
    
    /// Delay between restart attempts
    pub const RESTART_DELAY: Duration = Duration::from_secs(1);
    
    /// Log follow polling interval
    pub const LOG_FOLLOW_INTERVAL: Duration = Duration::from_millis(100);
    
    /// Process shutdown timeout (SIGTERM to SIGKILL)
    pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
    
    /// Default number of log lines to show
    pub const DEFAULT_LOG_LINES: usize = 50;
}
