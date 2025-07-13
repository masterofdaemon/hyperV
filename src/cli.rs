use clap::{Parser, Subcommand};
use crate::logs::LogType;

/// hyperV CLI application
#[derive(Parser)]
#[command(name = "hyperV")]
#[command(about = "A service manager for running binary files")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Create a new task
    New {
        /// Name of the task
        #[arg(short, long)]
        name: String,
        /// Path to the binary file
        #[arg(short, long)]
        binary: String,
        /// Environment variables (format: KEY=VALUE)
        #[arg(short, long)]
        env: Vec<String>,
        /// Working directory
        #[arg(short, long)]
        workdir: Option<String>,
        /// Auto-restart on failure
        #[arg(long)]
        auto_restart: bool,
        /// Arguments for the binary (must be the last option)
        #[arg(short, long, num_args = 1.., allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List all tasks
    List,
    /// Start a task
    Start {
        /// Task name or ID
        task: String,
    },
    /// Stop a task
    Stop {
        /// Task name or ID
        task: String,
    },
    /// Remove a task
    Remove {
        /// Task name or ID
        task: String,
    },
    /// Show task status
    Status {
        /// Task name or ID (optional, shows all if not specified)
        task: Option<String>,
    },
    /// Show task logs
    Logs {
        /// Task name or ID
        task: String,
        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
        /// Log type to show
        #[arg(short = 't', long, default_value = "stdout")]
        log_type: LogType,
        /// Follow logs in real-time (like tail -f)
        #[arg(short, long)]
        follow: bool,
    },
    /// Diagnose binary file issues
    Diagnose {
        /// Task name or ID
        task: String,
    },
    /// Run in daemon mode (monitor and auto-restart tasks)
    Daemon,
}

