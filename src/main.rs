use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "hyperV")]
#[command(about = "A service manager for running binary files")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new task
    New {
        /// Name of the task
        #[arg(short, long)]
        name: String,
        /// Path to the binary file
        #[arg(short, long)]
        binary: String,
        /// Arguments for the binary
        #[arg(short, long)]
        args: Vec<String>,
        /// Environment variables (format: KEY=VALUE)
        #[arg(short, long)]
        env: Vec<String>,
        /// Working directory
        #[arg(short, long)]
        workdir: Option<String>,
        /// Auto-restart on failure
        #[arg(long)]
        auto_restart: bool,
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
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Task {
    id: String,
    name: String,
    binary: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    workdir: Option<String>,
    auto_restart: bool,
    status: TaskStatus,
    created_at: String,
    pid: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum TaskStatus {
    Stopped,
    Running,
    Failed,
}

struct TaskManager {
    tasks: Vec<Task>,
    config_path: PathBuf,
    running_processes: HashMap<String, Child>,
}

impl TaskManager {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("hyperV");
        
        fs::create_dir_all(&config_dir)?;
        let config_path = config_dir.join("tasks.json");
        
        let tasks = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(TaskManager {
            tasks,
            config_path,
            running_processes: HashMap::new(),
        })
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.tasks)?;
        fs::write(&self.config_path, json)?;
        Ok(())
    }

    fn create_task(
        &mut self,
        name: String,
        binary: String,
        args: Vec<String>,
        env_vars: Vec<String>,
        workdir: Option<String>,
        auto_restart: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if task name already exists
        if self.tasks.iter().any(|t| t.name == name) {
            return Err(format!("Task with name '{}' already exists", name).into());
        }

        // Parse environment variables
        let mut env = HashMap::new();
        for env_var in env_vars {
            if let Some((key, value)) = env_var.split_once('=') {
                env.insert(key.to_string(), value.to_string());
            } else {
                return Err(format!("Invalid environment variable format: {}", env_var).into());
            }
        }

        let task = Task {
            id: Uuid::new_v4().to_string(),
            name,
            binary,
            args,
            env,
            workdir,
            auto_restart,
            status: TaskStatus::Stopped,
            created_at: chrono::Utc::now().to_rfc3339(),
            pid: None,
        };

        self.tasks.push(task);
        self.save()?;
        println!("Task created successfully!");
        Ok(())
    }

    fn list_tasks(&self) {
        if self.tasks.is_empty() {
            println!("No tasks configured.");
            return;
        }

        println!("{:<36} {:<20} {:<15} {:<30}", "ID", "NAME", "STATUS", "BINARY");
        println!("{}", "-".repeat(100));
        
        for task in &self.tasks {
            let status = match task.status {
                TaskStatus::Running => "🟢 Running",
                TaskStatus::Stopped => "🔴 Stopped",
                TaskStatus::Failed => "🟡 Failed",
            };
            
            println!(
                "{:<36} {:<20} {:<15} {:<30}",
                &task.id[..8],
                task.name,
                status,
                task.binary
            );
        }
    }

    fn find_task(&self, identifier: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        )
    }

    fn find_task_mut(&mut self, identifier: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        )
    }

    fn start_task(&mut self, identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
        let task = self.find_task(identifier)
            .ok_or(format!("Task '{}' not found", identifier))?
            .clone();

        if task.status == TaskStatus::Running {
            return Err("Task is already running".into());
        }

        let mut cmd = Command::new(&task.binary);
        cmd.args(&task.args);
        
        for (key, value) in &task.env {
            cmd.env(key, value);
        }

        if let Some(workdir) = &task.workdir {
            cmd.current_dir(workdir);
        }

        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());

        let child = cmd.spawn()?;
        let pid = child.id();

        self.running_processes.insert(task.id.clone(), child);
        
        if let Some(task_mut) = self.find_task_mut(identifier) {
            task_mut.status = TaskStatus::Running;
            task_mut.pid = Some(pid);
        }

        self.save()?;
        println!("Task '{}' started with PID {}", task.name, pid);
        Ok(())
    }

    fn stop_task(&mut self, identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
        let task = self.find_task(identifier)
            .ok_or(format!("Task '{}' not found", identifier))?
            .clone();

        if task.status != TaskStatus::Running {
            return Err("Task is not running".into());
        }

        if let Some(mut child) = self.running_processes.remove(&task.id) {
            child.kill()?;
            child.wait()?;
        }

        if let Some(task_mut) = self.find_task_mut(identifier) {
            task_mut.status = TaskStatus::Stopped;
            task_mut.pid = None;
        }

        self.save()?;
        println!("Task '{}' stopped", task.name);
        Ok(())
    }

    fn remove_task(&mut self, identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
        let task_index = self.tasks.iter().position(|t| 
            t.name == identifier || 
            t.id == identifier || 
            t.id.starts_with(identifier)
        ).ok_or(format!("Task '{}' not found", identifier))?;

        // Check if task is running and stop it first
        let is_running = self.tasks[task_index].status == TaskStatus::Running;
        if is_running {
            self.stop_task(identifier)?;
        }

        let task_name = self.tasks[task_index].name.clone();
        self.tasks.remove(task_index);
        self.save()?;
        println!("Task '{}' removed", task_name);
        Ok(())
    }

    fn show_status(&self, identifier: Option<&str>) {
        match identifier {
            Some(id) => {
                if let Some(task) = self.find_task(id) {
                    self.print_task_details(task);
                } else {
                    println!("Task '{}' not found", id);
                }
            }
            None => {
                if self.tasks.is_empty() {
                    println!("No tasks configured.");
                } else {
                    for task in &self.tasks {
                        self.print_task_details(task);
                        println!("{}", "-".repeat(50));
                    }
                }
            }
        }
    }

    fn print_task_details(&self, task: &Task) {
        println!("Task: {}", task.name);
        println!("ID: {}", task.id);
        println!("Binary: {}", task.binary);
        println!("Args: {:?}", task.args);
        println!("Status: {:?}", task.status);
        if let Some(pid) = task.pid {
            println!("PID: {}", pid);
        }
        println!("Auto-restart: {}", task.auto_restart);
        if let Some(workdir) = &task.workdir {
            println!("Working directory: {}", workdir);
        }
        if !task.env.is_empty() {
            println!("Environment variables:");
            for (key, value) in &task.env {
                println!("  {}={}", key, value);
            }
        }
        println!("Created: {}", task.created_at);
    }

    fn show_logs(&self, identifier: &str, _lines: usize) {
        if let Some(_task) = self.find_task(identifier) {
            println!("Log viewing functionality will be implemented in future versions.");
            println!("For now, you can check system logs or redirect output when starting tasks.");
        } else {
            println!("Task '{}' not found", identifier);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut task_manager = TaskManager::new()?;

    match cli.command {
        Commands::New {
            name,
            binary,
            args,
            env,
            workdir,
            auto_restart,
        } => {
            task_manager.create_task(name, binary, args, env, workdir, auto_restart)?;
        }
        Commands::List => {
            task_manager.list_tasks();
        }
        Commands::Start { task } => {
            task_manager.start_task(&task)?;
        }
        Commands::Stop { task } => {
            task_manager.stop_task(&task)?;
        }
        Commands::Remove { task } => {
            task_manager.remove_task(&task)?;
        }
        Commands::Status { task } => {
            task_manager.show_status(task.as_deref());
        }
        Commands::Logs { task, lines } => {
            task_manager.show_logs(&task, lines);
        }
    }

    Ok(())
}
