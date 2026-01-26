//! Task manager for hyperV
//! 
//! High-level task management operations including CRUD operations,
//! process lifecycle management, and coordination between modules.

use crate::config::Config;
use crate::error::{HyperVError, Result};
use crate::logs::{LogManager, LogType};
use crate::process::{ProcessManager, diagnose_binary};
use crate::task::{Task, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;
use sysinfo::{System, Pid};

#[derive(Serialize, Deserialize, Clone)]
struct RunningTask {
    task_id: String,
    pid: u32,
}

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
    fn get_process_memory_mb(sys: &mut System, pid: u32) -> u64 {
        let pid = Pid::from_u32(pid);
        if let Some(proc_) = sys.process(pid) {
            // memory() returns bytes in sysinfo 0.30
            let bytes = proc_.memory();
            return (bytes / (1024 * 1024)) as u64; // Convert bytes to MB
        }
        0
    }

    /// Create a new task manager
    pub fn new() -> Result<Self> {
        let config = Config::new()?;
        let process_manager = ProcessManager::new();
        
        let mut manager = Self {
            tasks: Vec::new(),
            config,
            process_manager,
        };
        
        // Load existing tasks (with locking)
        if let Err(e) = manager.load() {
            eprintln!("⚠️  Failed to load tasks: {}", e);
            // If load fails, we start with empty tasks. 
            // In a robust system we might want to backup here, but load() has error handling now.
            // Let's rely on load()'s integrity.
        }

        // Hydrate status from running_tasks.json
        if manager.config.running_tasks_file.exists() {
            let content = fs::read_to_string(&manager.config.running_tasks_file)
                .map_err(HyperVError::Io)?;
            if let Ok(running_tasks) = serde_json::from_str::<Vec<RunningTask>>(&content) {
                for running_task in running_tasks {
                    use sysinfo::{System, Pid};
                    // Use sysinfo to check process
                    let mut sys = System::new();
                    let pid = Pid::from_u32(running_task.pid);
                    if sys.refresh_process(pid) {
                         if let Some(task) = manager.tasks.iter_mut().find(|t| t.id == running_task.task_id) {
                            task.set_status(TaskStatus::Running);
                            task.set_pid(Some(running_task.pid));
                        }
                    }
                }
            }
        }

        Ok(manager)
    }

    /// Load tasks from configuration file
    pub(crate) fn load(&mut self) -> Result<()> {
        if !self.config.tasks_file.exists() {
            return Ok(());
        }

        let file = fs::File::open(&self.config.tasks_file).map_err(HyperVError::Io)?;
        
        // Lock shared read
        use fs2::FileExt;
        if let Err(e) = file.try_lock_shared() {
             // If we can't get a shared lock, maybe someone is writing.
             // We can wait or hard fail. Let's wait a bit.
             // Actually, for simplicity, let's just wait or fail.
             // Given it's a CLI tool, blocking is okay.
             file.lock_shared().map_err(HyperVError::Io)?;
        }

        let reader = std::io::BufReader::new(&file);
        let tasks: Vec<Task> = serde_json::from_reader(reader)
            .map_err(|e| HyperVError::Serialization(e.to_string()))?;
        
        // Unlock happens automatically when file is dropped
        self.tasks = tasks;
        Ok(())
    }

    /// Save tasks to configuration file
    pub(crate) fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.tasks)
            .map_err(|e| HyperVError::Serialization(e.to_string()))?;
        
        // To prevent races, we should acquire a lock.
        // But since we are writing to a temp file then renaming, the rename is atomic.
        // However, we might have read STALE data if we didn't lock during read-modify-write.
        // The proper way is: lock FILE, read, modify, write, unlock.
        // But load() and save() are separate methods in this struct.
        // This suggests the TaskManager should perhaps hold the lock?
        // Or we use a separate lock file "tasks.lock" that we check.
        
        // For now, let's implement a lock on the directory or a separate lock file to serialize writes.
        let lock_path = self.config.tasks_file.with_extension("lock");
        let lock_file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)
            .map_err(HyperVError::Io)?;
            
        use fs2::FileExt;
        lock_file.lock_exclusive().map_err(HyperVError::Io)?;

        // Now we really should reload data to be safe, but that changes the architecture significantly 
        // (functions would need to accept closures for modifications).
        // Let's at least protect the write phase.
        
        // Write atomically: write to temp file then rename over the original.
        let tmp_path = self.config.tasks_file.with_extension("json.tmp");
        fs::write(&tmp_path, json.as_bytes()).map_err(HyperVError::Io)?;

        // Backup previous file if exists
        if self.config.tasks_file.exists() {
            let backup_path = self.config.tasks_file.with_extension("json.prev");
            let _ = fs::copy(&self.config.tasks_file, &backup_path);
        }

        fs::rename(&tmp_path, &self.config.tasks_file).map_err(HyperVError::Io)?;
        
        // Unlock happens when lock_file is dropped
        Ok(())
    }

    fn save_running_tasks(&self) -> Result<()> {
        let running_tasks: Vec<RunningTask> = self.tasks.iter()
            .filter(|t| t.status == TaskStatus::Running && t.pid.is_some())
            .map(|t| RunningTask {
                task_id: t.id.clone(),
                pid: t.pid.expect("Task status is Running but PID is None"),
            })
            .collect();

        let json = serde_json::to_string_pretty(&running_tasks)
            .map_err(|e| HyperVError::Serialization(e.to_string()))?;

        // Write atomically: write to temp file then rename
        let tmp_path = self.config.running_tasks_file.with_extension("json.tmp");
        fs::write(&tmp_path, json.as_bytes()).map_err(HyperVError::Io)?;
        fs::rename(&tmp_path, &self.config.running_tasks_file).map_err(HyperVError::Io)?;

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

        println!("{:<36} {:<18} {:<15} {:<11} {:<20} {:<30}", "ID", "NAME", "STATUS", "MEM(MB)", "STARTED", "BINARY");
        println!("{}", "-".repeat(140));

        let mut sys = System::new();
        sys.refresh_processes();

        for task in &self.tasks {
            let status_display = task.status.display_with_icon();
            // Memory usage in MB if running
            let mem_mb = if let (TaskStatus::Running, Some(pid)) = (&task.status, task.pid) {
                Self::get_process_memory_mb(&mut sys, pid)
            } else { 0 };

            let started = task
                .last_started
                .as_deref()
                .unwrap_or("-");
            println!(
                "{:<36} {:<18} {:<15} {:<11} {:<20} {:<30}",
                &task.id[..8],
                task.name,
                status_display,
                mem_mb,
                started,
                task.binary
            );
        }
    }

    /// Find a task by identifier (name, ID, or partial ID)
    pub(crate) fn find_task(&self, identifier: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        )
    }

    /// Find a mutable task by identifier
    pub(crate) fn find_task_mut(&mut self, identifier: &str) -> Option<&mut Task> {
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
                    task_mut.clear_suppress_restart();
                }

                self.save()?;
                self.save_running_tasks()?;
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
        let (task_name, task_id, pid) = {
            let task = self.find_task(identifier)
                .ok_or_else(|| HyperVError::TaskNotFound(identifier.to_string()))?;

            if task.status != TaskStatus::Running {
                println!("ℹ️  Task \"{}\" is already stopped", task.name);
                return Ok(());
            }
            (task.name.clone(), task.id.clone(), task.pid)
        };

        if let Some(pid) = pid {
            if !self.process_manager.is_process_running(pid) {
                println!("ℹ️  Process {} for task \"{}\" has already terminated", pid, task_name);
            } else {
                println!("🛑 Stopping task \"{}\" (PID: {})...", task_name, pid);
                self.process_manager.stop_task(&task_id, pid)?;
            }
        }

        if let Some(task) = self.find_task_mut(identifier) {
            task.set_status(TaskStatus::Stopped);
            task.suppress_restart = true;
            task.clear_pid();
        }

        self.save()?;
        self.save_running_tasks()?;
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
        self.save_running_tasks()?;
        
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
 
        // Reload tasks from disk to pick up external changes (like suppression on stop)
        if self.config.tasks_file.exists() {
            if let Ok(content) = fs::read_to_string(&self.config.tasks_file) {
                if let Ok(tasks_on_disk) = serde_json::from_str::<Vec<Task>>(&content) {
                    self.tasks = tasks_on_disk;
                }
            }
        }

        let tasks_to_restart: Vec<String> = self.tasks
            .iter()
            .filter(|task| {
                task.auto_restart &&
                !task.suppress_restart &&
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
                        task.set_status(TaskStatus::Failed);
                        task.clear_pid();
                        updated = true;
                    }
                }
            }
        }
        
        if updated {
            self.save()?;
            self.save_running_tasks()?;
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
        // Reload tasks from disk to incorporate external updates (e.g., stop suppression)
        if self.config.tasks_file.exists() {
            if let Ok(content) = fs::read_to_string(&self.config.tasks_file) {
                if let Ok(tasks_on_disk) = serde_json::from_str::<Vec<Task>>(&content) {
                    self.tasks = tasks_on_disk;
                }
            }
        }

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
            self.save_running_tasks()?;
        }
        
        Ok(())
    }

    /// Whether any task has auto-restart enabled
    pub fn any_autorestart_enabled(&self) -> bool {
        self.tasks.iter().any(|t| t.auto_restart)
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize task manager")
    }
}
