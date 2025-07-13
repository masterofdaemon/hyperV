//! Task manager for hyperV
//! 
//! High-level task management operations including CRUD operations,
//! process lifecycle management, and coordination between modules.

use crate::config::Config;
use crate::error::{HyperVError, Result};
use crate::logs::{LogManager, LogType};
use crate::process::{ProcessManager, diagnose_binary};
use crate::task::{Task, TaskStatus};
use serde_json;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;

/// Main task manager that coordinates all operations
pub struct TaskManager {
    /// Task configuration
    tasks: Vec<Task>,
    /// Configuration manager
    config: Config,
    /// Process manager
    process_manager: ProcessManager,
}

impl TaskManager {
    /// Create a new task manager
    pub fn new() -> Result<Self> {
        let config = Config::new()?;
        
        // Load existing tasks
        let tasks = if config.tasks_file.exists() {
            let content = fs::read_to_string(&config.tasks_file)
                .map_err(HyperVError::Io)?;
            serde_json::from_str(&content)
                .unwrap_or_else(|_| Vec::new())
        } else {
            Vec::new()
        };

        Ok(Self {
            tasks,
            config,
            process_manager: ProcessManager::new(),
        })
    }

    /// Save tasks to configuration file
    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.tasks)
            .map_err(|e| HyperVError::Serialization(e.to_string()))?;
        
        fs::write(&self.config.tasks_file, json)
            .map_err(HyperVError::Io)?;
        
        Ok(())
    }

    /// Create a new task
    pub fn create_task(
        &mut self,
        name: String,
        binary: String,
        args: Vec<String>,
        env_vars: Vec<String>,
        workdir: Option<String>,
        auto_restart: bool,
    ) -> Result<()> {
        // Check if task name already exists
        if self.tasks.iter().any(|t| t.name == name) {
            return Err(HyperVError::TaskExists(name));
        }

        // Parse environment variables from command line
        let mut env = HashMap::new();
        for env_var in env_vars {
            if let Some((key, value)) = env_var.split_once('=') {
                env.insert(key.to_string(), value.to_string());
            } else {
                return Err(HyperVError::InvalidEnvVar(env_var));
            }
        }

        // Load environment variables from .env file in workdir
        if let Some(ref workdir) = workdir {
            let env_file_path = std::path::Path::new(workdir).join(".env");
            if env_file_path.exists() {
                if let Ok(lines) = std::fs::read_to_string(&env_file_path) {
                    for line in lines.lines() {
                        if let Some((key, value)) = line.split_once('=') {
                            // Command-line env vars take precedence
                            if !env.contains_key(key) {
                                env.insert(key.to_string(), value.to_string());
                            }
                        }
                    }
                }
            }
        }

        let id = Uuid::new_v4().to_string();
        
        // Ensure log directory exists
        self.config.ensure_task_log_dir(&id)?;
        
        let stdout_log_path = self.config.stdout_log_path(&id);
        let stderr_log_path = self.config.stderr_log_path(&id);
        
        let task = Task::new(
            id,
            name,
            binary,
            args,
            env,
            workdir,
            auto_restart,
            Some(stdout_log_path.to_string_lossy().to_string()),
            Some(stderr_log_path.to_string_lossy().to_string()),
        );

        self.tasks.push(task);
        self.save()?;
        println!("✅ Task created successfully!");
        Ok(())
    }

    /// List all tasks
    pub fn list_tasks(&mut self) {
        // Refresh task statuses before listing
        let _ = self.refresh_task_statuses();
        
        if self.tasks.is_empty() {
            println!("No tasks configured.");
            return;
        }

        println!("{:<36} {:<20} {:<15} {:<30}", "ID", "NAME", "STATUS", "BINARY");
        println!("{}", "-".repeat(100));
        
        for task in &self.tasks {
            let status_display = task.status.display_with_icon();
            println!(
                "{:<36} {:<20} {:<15} {:<30}",
                &task.id[..8],
                task.name,
                status_display,
                task.binary
            );
        }
    }

    /// Find a task by identifier (name, ID, or partial ID)
    fn find_task(&self, identifier: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        )
    }

    /// Find a mutable task by identifier
    fn find_task_mut(&mut self, identifier: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        )
    }

    /// Start a task
    pub fn start_task(&mut self, identifier: &str) -> Result<()> {
        let task = self.find_task(identifier)
            .ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?.clone();


        // Check if task is already running
        if task.status == TaskStatus::Running {
            if let Some(pid) = task.pid {
                if self.process_manager.is_process_running(pid) {
                    return Err(HyperVError::TaskAlreadyRunning(task.name));
                } else {
                    // Process died, update status
                    if let Some(task_mut) = self.find_task_mut(identifier) {
                        task_mut.set_status(TaskStatus::Failed);
                        task_mut.clear_pid();
                    }
                    self.save()?;
                }
            }
        }

        // Validate working directory
        if let Some(ref workdir) = task.workdir {
            if !std::path::Path::new(workdir).exists() {
                return Err(HyperVError::WorkdirNotFound(workdir.clone()));
            }
        }

        // Get log paths
        let stdout_path = self.config.stdout_log_path(&task.id);
        let stderr_path = self.config.stderr_log_path(&task.id);

        // Rotate logs if needed
        LogManager::rotate_log_if_needed(&stdout_path)?;
        LogManager::rotate_log_if_needed(&stderr_path)?;

        println!("🚀 Starting task \"{}\" with binary: {}", task.name, task.binary);
        if !task.args.is_empty() {
            println!("   Arguments: {:?}", task.args);
        }
        if !task.env.is_empty() {
            println!("   Environment variables: {} vars", task.env.len());
        }
        if let Some(ref workdir) = task.workdir {
            println!("   Working directory: {}", workdir);
        }

        // Clone the task's env and load from .env file
        let mut task_env = task.env.clone();
        if let Some(ref workdir) = task.workdir {
            let env_file_path = std::path::Path::new(workdir).join(".env");
            if env_file_path.exists() {
                if let Ok(lines) = std::fs::read_to_string(&env_file_path) {
                    for line in lines.lines() {
                        if let Some((key, value)) = line.split_once('=') {
                            // Task-specific env vars take precedence
                            if !task_env.contains_key(key) {
                                task_env.insert(key.to_string(), value.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Start the process
        match self.process_manager.start_task(&task, &task_env, &stdout_path, &stderr_path) {
            Ok(pid) => {
                // Update task state
                if let Some(task_mut) = self.find_task_mut(identifier) {
                    task_mut.set_status(TaskStatus::Running);
                    task_mut.set_pid(Some(pid));
                    task_mut.set_last_started();
                }

                self.save()?;
                println!("✅ Task \"{}\" started successfully with PID {}", task.name, pid);
                Ok(())
            }
            Err(e) => {
                // Update task state to failed
                if let Some(task_mut) = self.find_task_mut(identifier) {
                    task_mut.set_status(TaskStatus::Failed);
                }
                self.save()?;
                Err(e)
            }
        }
    }

    /// Stop a task
    pub fn stop_task(&mut self, identifier: &str) -> Result<()> {
        let task = self.find_task(identifier)
            .ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?.clone();


        let task_name = task.name.clone();
        let task_id = task.id.clone();

        // Check if task is marked as running but process doesn't exist
        if task.status == TaskStatus::Running {
            if let Some(pid) = task.pid {
                if !self.process_manager.is_process_running(pid) {
                    // Process is already dead, just update the status
                    println!("ℹ️  Process {} for task \"{}\" has already terminated", pid, task_name);
                    if let Some(task_mut) = self.find_task_mut(identifier) {
                        task_mut.set_status(TaskStatus::Stopped);
                        task_mut.clear_pid();
                    }
                    self.save()?;
                    println!("✅ Task \"{}\" status updated to stopped", task_name);
                    return Ok(());
                }
                
                // Process is still running, try to stop it
                println!("🛑 Stopping task \"{}\" (PID: {})...", task_name, pid);
                self.process_manager.stop_task(&task_id, pid)?;
            }
        } else {
            println!("ℹ️  Task \"{}\" is already stopped", task_name);
            return Ok(());
        }

        // Update task state
        if let Some(task_mut) = self.find_task_mut(identifier) {
            task_mut.set_status(TaskStatus::Stopped);
            task_mut.clear_pid();
        }

        self.save()?;
        println!("✅ Task \"{}\" stopped", task_name);
        Ok(())
    }

    /// Remove a task
    pub fn remove_task(&mut self, identifier: &str) -> Result<()> {
        let task_index = self.tasks.iter().position(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        ).ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?;

        // Check if task is running and stop it first
        let is_running = self.tasks[task_index].status == TaskStatus::Running;
        if is_running {
            self.stop_task(identifier)?;
        }

        let task_name = self.tasks[task_index].name.clone();
        self.tasks.remove(task_index);
        self.save()?;
        
        println!("✅ Task \"{}\" removed", task_name);
        Ok(())
    }

    /// Show task status
    pub fn show_status(&self, identifier: Option<&str>) {
        match identifier {
            Some(id) => {
                if let Some(task) = self.find_task(id) {
                    task.print_details();
                } else {
                    println!("❌ Task \"{}\" not found", id);
                }
            }
            None => {
                if self.tasks.is_empty() {
                    println!("No tasks configured.");
                } else {
                    for task in &self.tasks {
                        task.print_details();
                        println!("{}", "-".repeat(50));
                    }
                }
            }
        }
    }

    /// Show task logs
    pub fn show_logs(
        &self,
        identifier: &str,
        lines: usize,
        log_type: LogType,
        follow: bool,
    ) -> Result<()> {
        let task = self.find_task(identifier)
            .ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?;

        let stdout_path = self.config.stdout_log_path(&task.id);
        let stderr_path = self.config.stderr_log_path(&task.id);

        LogManager::show_logs(&stdout_path, &stderr_path, log_type, lines, follow)
    }

    /// Diagnose a task's binary
    pub fn diagnose_task(&self, identifier: &str) -> Result<()> {
        let task = self.find_task(identifier)
            .ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?;

        println!("🔍 Diagnosing task: {}", task.name);
        println!("---------------------------------------------------");
        
        // Diagnose the binary
        diagnose_binary(&task.binary)?;

        // Show task configuration
        println!("
⚙️  Task Configuration:");
        task.print_details();

        Ok(())
    }

    /// Check and restart failed tasks with auto-restart enabled
    pub fn check_and_restart_tasks(&mut self) -> Result<()> {
        use crate::constants::{MAX_RESTART_ATTEMPTS, RESTART_DELAY};
        
        let tasks_to_restart: Vec<String> = self.tasks
            .iter()
            .filter(|task| {
                task.auto_restart && 
                task.status == TaskStatus::Failed && 
                task.restart_count <= MAX_RESTART_ATTEMPTS
            })
            .map(|task| task.id.clone())
            .collect();

        for task_id in tasks_to_restart {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                println!("🔄 Auto-restarting failed task: {} (attempt {}/{})", 
                    task.name, task.restart_count + 1, MAX_RESTART_ATTEMPTS);
                
                task.increment_restart_count();
                let task_name = task.name.clone();
                self.save()?;
                
                // Small delay before restart
                std::thread::sleep(RESTART_DELAY);
                
                if let Err(e) = self.start_task(&task_name) {
                    println!("❌ Failed to auto-restart task \"{}\": {}", task_name, e);
                    // Mark as failed again if restart fails
                    if let Some(task_mut) = self.find_task_mut(&task_name) {
                        task_mut.set_status(TaskStatus::Failed);
                    }
                    self.save()?;
                } else {
                    println!("✅ Task \"{}\" restarted successfully", task_name);
                }
            }
        }
        
        Ok(())
    }

    /// Refresh task statuses by checking if running processes are still alive
    pub fn refresh_task_statuses(&mut self) -> Result<()> {
        let mut updated = false;
        
        for task in &mut self.tasks {
            if task.status == TaskStatus::Running {
                if let Some(pid) = task.pid {
                    if !self.process_manager.is_process_running(pid) {
                        // Process has terminated, update status
                        task.set_status(TaskStatus::Stopped);
                        task.clear_pid();
                        updated = true;
                    }
                }
            }
        }
        
        if updated {
            self.save()?;
        }
        
        Ok(())
    }

    /// Get the number of tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get the number of running tasks
    pub fn running_task_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Running).count()
    }

    /// Get the number of tasks with auto-restart enabled
    pub fn tasks_with_autorestart_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.auto_restart).count()
    }

    /// Clean up zombie processes and update task states
    pub fn cleanup(&mut self) -> Result<()> {
        let exit_codes = self.process_manager.cleanup_zombies();
        
        // Update task states for processes that are no longer running
        let mut changed = false;
        for task in &mut self.tasks {
            if task.status == TaskStatus::Running {
                if let Some(pid) = task.pid {
                    if !self.process_manager.is_process_running(pid) {
                        // Check if we have an exit code for this task
                        if let Some(&exit_code) = exit_codes.get(&task.id) {
                            task.set_exit_code(Some(exit_code));
                            println!("ℹ️  Task \"{}\" exited with code {}", task.name, exit_code);
                        }
                        
                        task.set_status(TaskStatus::Failed);
                        task.clear_pid();
                        changed = true;
                    }
                }
            }
        }
        
        if changed {
            self.save()?;
        }
        
        Ok(())
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize task manager")
    }
}
