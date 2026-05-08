use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Task status enumeration
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Stopped,
    Running,
    Failed,
}

impl TaskStatus {
    /// Get status display with icon
    pub fn display_with_icon(&self) -> &'static str {
        match self {
            TaskStatus::Stopped => "🔴 Stopped",
            TaskStatus::Running => "🟢 Running",
            TaskStatus::Failed => "🟡 Failed",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_with_icon())
    }
}

/// Task configuration and state
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub workdir: Option<String>,
    pub auto_restart: bool,
    pub status: TaskStatus,
    pub created_at: String,
    pub pid: Option<u32>,
    /// Best-effort process identity (used to detect PID reuse). On Unix, this typically matches
    /// sysinfo's start_time() for the process at the moment we spawned it.
    #[serde(default)]
    pub pid_start_time: Option<u64>,
    pub stdout_log_path: Option<String>,
    pub stderr_log_path: Option<String>,
    pub last_started: Option<String>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    #[serde(default)]
    pub suppress_restart: bool,
}

impl Task {
    /// Create a new task with the given parameters
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        binary: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        workdir: Option<String>,
        auto_restart: bool,
        stdout_log_path: Option<String>,
        stderr_log_path: Option<String>,
    ) -> Self {
        Task {
            id,
            name,
            binary,
            args,
            env,
            workdir,
            auto_restart,
            status: TaskStatus::Stopped,
            created_at: chrono::Utc::now().to_rfc3339(),
            pid: None,
            pid_start_time: None,
            stdout_log_path,
            stderr_log_path,
            last_started: None,
            restart_count: 0,
            last_exit_code: None,
            suppress_restart: false,
        }
    }

    /// Set task status
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    /// Set task PID
    pub fn set_pid(&mut self, pid: Option<u32>) {
        self.pid = pid;
    }

    /// Set task PID start time (used to detect PID reuse)
    pub fn set_pid_start_time(&mut self, pid_start_time: Option<u64>) {
        self.pid_start_time = pid_start_time;
    }

    /// Clear task PID
    pub fn clear_pid(&mut self) {
        self.pid = None;
        self.pid_start_time = None;
    }

    /// Set last started timestamp to now
    pub fn set_last_started(&mut self) {
        self.last_started = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Clear the suppress-restart flag (used for explicit user starts)
    pub fn clear_suppress_restart(&mut self) {
        self.suppress_restart = false;
    }

    /// Increment restart count
    pub fn increment_restart_count(&mut self) {
        self.restart_count += 1;
    }

    /// Set last exit code
    pub fn set_exit_code(&mut self, exit_code: Option<i32>) {
        self.last_exit_code = exit_code;
    }

    /// Print detailed task information
    pub fn print_details(&self) {
        println!("Task: {}", self.name);
        println!("ID: {}", self.id);
        println!("Binary: {}", self.binary);
        println!("Args: {:?}", self.args);
        println!("Status: {}", self.status);

        if let Some(pid) = self.pid {
            println!("PID: {}", pid);
        }

        if let Some(exit_code) = self.last_exit_code {
            println!("Last exit code: {}", exit_code);
        }

        println!(
            "Auto-restart: {} (restarts: {})",
            self.auto_restart, self.restart_count
        );

        if let Some(workdir) = &self.workdir {
            println!("Working directory: {}", workdir);
        }

        if !self.env.is_empty() {
            println!("Environment variables:");
            for (key, value) in &self.env {
                println!("  {}={}", key, value);
            }
        }

        println!("Created: {}", self.created_at);

        if let Some(last_started) = &self.last_started {
            println!("Last started: {}", last_started);
        }
    }
}
