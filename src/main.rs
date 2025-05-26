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
        /// Log type to show
        #[arg(short, long, default_value = "stdout")]
        log_type: String, // "stdout" or "stderr"
    },
    /// Diagnose binary file issues
    Diagnose {
        /// Task name or ID
        task: String,
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
    stdout_log_path: Option<String>,
    stderr_log_path: Option<String>,
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

        let id = Uuid::new_v4().to_string();
        let config_dir = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("hyperV");
        let logs_dir = config_dir.join("logs").join(&id);
        fs::create_dir_all(&logs_dir)?;
        let stdout_log_path = logs_dir.join("stdout.log");
        let stderr_log_path = logs_dir.join("stderr.log");
        let task = Task {
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
            stdout_log_path: Some(stdout_log_path.to_string_lossy().to_string()),
            stderr_log_path: Some(stderr_log_path.to_string_lossy().to_string()),
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

        // Check if binary exists and is executable
        let binary_path = std::path::Path::new(&task.binary);
        if !binary_path.exists() {
            return Err(format!("❌ Binary file does not exist: {}", task.binary).into());
        }

        // Check if file is executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&task.binary)?;
            let permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                return Err(format!("❌ Binary file is not executable: {}\n💡 Fix with: chmod +x {}", task.binary, task.binary).into());
            }
        }

        // Check if it's a script and has proper shebang
        let mut file = std::fs::File::open(&task.binary)?;
        let mut buffer = [0; 512];
        use std::io::Read;
        let bytes_read = file.read(&mut buffer).unwrap_or(0);
        
        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 { // "#!" shebang
            println!("📝 Detected script file with shebang");
        } else if buffer.iter().take(bytes_read).all(|&b| b.is_ascii() && b != 0) {
            println!("⚠️  Detected text file without shebang");
            println!("💡 If this is a shell script, add '#!/bin/bash' as the first line");
        }

        let mut cmd = Command::new(&task.binary);
        cmd.args(&task.args);
        for (key, value) in &task.env {
            cmd.env(key, value);
        }
        if let Some(workdir) = &task.workdir {
            cmd.current_dir(workdir);
        }
        // Setup log files
        let stdout_log_path;
        let stderr_log_path;
        
        if let (Some(out), Some(err)) = (&task.stdout_log_path, &task.stderr_log_path) {
            stdout_log_path = out.clone();
            stderr_log_path = err.clone();
        } else {
            // Backward compatibility for old tasks
            let config_dir = dirs::config_dir()
                .ok_or("Could not find config directory")?
                .join("hyperV");
            let logs_dir = config_dir.join("logs").join(&task.id);
            fs::create_dir_all(&logs_dir)?;
            let out = logs_dir.join("stdout.log");
            let err = logs_dir.join("stderr.log");
            stdout_log_path = out.to_string_lossy().to_string();
            stderr_log_path = err.to_string_lossy().to_string();
            
            if let Some(task_mut) = self.find_task_mut(identifier) {
                task_mut.stdout_log_path = Some(stdout_log_path.clone());
                task_mut.stderr_log_path = Some(stderr_log_path.clone());
                self.save()?;
            }
        }
        let stdout_file = fs::OpenOptions::new().create(true).append(true).open(&stdout_log_path)?;
        let stderr_file = fs::OpenOptions::new().create(true).append(true).open(&stderr_log_path)?;
        cmd.stdout(Stdio::from(stdout_file));
        cmd.stderr(Stdio::from(stderr_file));

        println!("🚀 Starting task '{}' with binary: {}", task.name, task.binary);
        if !task.args.is_empty() {
            println!("   Arguments: {:?}", task.args);
        }
        if !task.env.is_empty() {
            println!("   Environment variables: {} vars", task.env.len());
        }
        if let Some(ref workdir) = task.workdir {
            println!("   Working directory: {}", workdir);
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.running_processes.insert(task.id.clone(), child);
                
                if let Some(task_mut) = self.find_task_mut(identifier) {
                    task_mut.status = TaskStatus::Running;
                    task_mut.pid = Some(pid);
                }

                self.save()?;
                println!("✅ Task '{}' started successfully with PID {}", task.name, pid);
            }
            Err(e) => {
                if let Some(task_mut) = self.find_task_mut(identifier) {
                    task_mut.status = TaskStatus::Failed;
                }
                self.save()?;
                return Err(format!("❌ Failed to start task '{}': {}\n\n💡 Troubleshooting tips:\n   1. Check if file exists: ls -la {}\n   2. Make executable: chmod +x {}\n   3. If it's a script, ensure it has shebang: head -1 {}\n   4. Test manually: {}\n   5. Use 'hyperV diagnose {}' for detailed analysis", 
                    task.name, e, task.binary, task.binary, task.binary, task.binary, task.name).into());
            }
        }

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

    fn show_logs(&self, identifier: &str, lines: usize, log_type: &str) {
        if let Some(task) = self.find_task(identifier) {
            let log_paths = match log_type {
                "stdout" => task.stdout_log_path.as_ref().map(|p| vec![p]),
                "stderr" => task.stderr_log_path.as_ref().map(|p| vec![p]),
                "both" | _ => {
                    let mut paths = Vec::new();
                    if let Some(stdout) = &task.stdout_log_path {
                        paths.push(stdout);
                    }
                    if let Some(stderr) = &task.stderr_log_path {
                        paths.push(stderr);
                    }
                    if paths.is_empty() { None } else { Some(paths) }
                }
            };

            if let Some(paths) = log_paths {
                for (i, log_path) in paths.iter().enumerate() {
                    if paths.len() > 1 {
                        println!("=== {} ===", if i == 0 { "STDOUT" } else { "STDERR" });
                    }
                    
                    let path = std::path::Path::new(log_path);
                    if !path.exists() {
                        println!("Log file does not exist: {}", log_path);
                        continue;
                    }
                    
                    let content = match fs::read_to_string(path) {
                        Ok(c) => c,
                        Err(e) => {
                            println!("Failed to read log file: {}", e);
                            continue;
                        }
                    };
                    
                    let lines_vec: Vec<&str> = content.lines().collect();
                    let start = if lines_vec.len() > lines { lines_vec.len() - lines } else { 0 };
                    for line in &lines_vec[start..] {
                        println!("{}", line);
                    }
                    
                    if paths.len() > 1 && i < paths.len() - 1 {
                        println!(); // Add blank line between stdout and stderr
                    }
                }
            } else {
                println!("No log files configured for this task.");
            }
        } else {
            println!("Task '{}' not found", identifier);
        }
    }

    fn diagnose_task(&self, identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
        let task = self.find_task(identifier)
            .ok_or(format!("Task '{}' not found", identifier))?;

        println!("🔍 Diagnosing task: {}", task.name);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        let binary_path = std::path::Path::new(&task.binary);
        
        // Check if file exists
        if !binary_path.exists() {
            println!("❌ File does not exist: {}", task.binary);
            println!("💡 Make sure the path is correct and the file exists");
            return Ok(());
        }
        println!("✅ File exists: {}", task.binary);

        // Check file metadata
        let metadata = std::fs::metadata(&task.binary)?;
        println!("📊 File size: {} bytes", metadata.len());
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = metadata.permissions();
            let mode = permissions.mode();
            println!("🔐 Permissions: {:o}", mode & 0o777);
            
            if mode & 0o111 == 0 {
                println!("❌ File is not executable");
                println!("💡 Fix with: chmod +x {}", task.binary);
            } else {
                println!("✅ File is executable");
            }
        }

        // Check file type and content
        let mut file = std::fs::File::open(&task.binary)?;
        let mut buffer = [0; 512];
        use std::io::Read;
        let bytes_read = file.read(&mut buffer)?;
        
        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 { // "#!"
            let shebang_line = String::from_utf8_lossy(&buffer[..bytes_read])
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            println!("✅ Script with shebang: {}", shebang_line);
            
            // Check if shebang interpreter exists
            let interpreter = shebang_line.trim_start_matches("#!")
                .split_whitespace()
                .next()
                .unwrap_or("");
            if !interpreter.is_empty() {
                let interpreter_path = std::path::Path::new(interpreter);
                if interpreter_path.exists() {
                    println!("✅ Interpreter exists: {}", interpreter);
                } else {
                    println!("❌ Interpreter not found: {}", interpreter);
                    println!("💡 Install the interpreter or fix the shebang path");
                }
            }
        } else if bytes_read >= 4 && buffer[..4] == [0x7f, 0x45, 0x4c, 0x46] { // ELF magic
            println!("✅ ELF binary file");
        } else if bytes_read >= 4 && buffer[..4] == [0xfe, 0xed, 0xfa, 0xce] { // Mach-O magic (little endian)
            println!("✅ Mach-O binary file (macOS)");
        } else if bytes_read >= 4 && buffer[..4] == [0xce, 0xfa, 0xed, 0xfe] { // Mach-O magic (big endian)  
            println!("✅ Mach-O binary file (macOS)");
        } else if buffer.iter().take(bytes_read).all(|&b| b.is_ascii() && b != 0) {
            println!("⚠️  Text file without shebang");
            println!("💡 If this is a script, add a shebang line:");
            println!("   #!/bin/bash          (for bash scripts)");
            println!("   #!/bin/sh            (for shell scripts)");
            println!("   #!/usr/bin/env python3  (for Python scripts)");
            println!("   #!/usr/bin/env node  (for Node.js scripts)");
        } else {
            println!("❓ Binary file (unknown format)");
        }

        // Show first few lines if it's a text file
        if buffer.iter().take(bytes_read).all(|&b| b.is_ascii() && b != 0) {
            println!("\n📄 First few lines of file:");
            let content = String::from_utf8_lossy(&buffer[..bytes_read]);
            for (i, line) in content.lines().take(5).enumerate() {
                println!("   {}: {}", i + 1, line);
            }
        }

        // Test basic execution
        println!("\n🧪 Testing basic execution...");
        let mut test_command = std::process::Command::new(&task.binary);
        test_command.arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        
        match test_command.spawn() {
            Ok(mut child) => {
                match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            println!("✅ Binary executes successfully");
                        } else {
                            println!("⚠️  Binary runs but exits with non-zero status");
                        }
                    }
                    Err(e) => println!("⚠️  Binary execution test failed: {}", e),
                }
            }
            Err(e) => {
                println!("❌ Cannot execute binary: {}", e);
                
                // Provide specific suggestions based on error
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        println!("💡 Permission denied - try: chmod +x {}", task.binary);
                    }
                    std::io::ErrorKind::NotFound => {
                        println!("💡 File not found or missing interpreter");
                    }
                    _ => {
                        println!("💡 Error details: {}", e);
                        println!("💡 Try running manually: {}", task.binary);
                    }
                }
            }
        }

        // Show task configuration
        println!("\n⚙️  Task Configuration:");
        println!("   Name: {}", task.name);
        println!("   Binary: {}", task.binary);
        if !task.args.is_empty() {
            println!("   Arguments: {:?}", task.args);
        }
        if !task.env.is_empty() {
            println!("   Environment variables:");
            for (key, value) in &task.env {
                println!("     {}={}", key, value);
            }
        }
        if let Some(ref workdir) = task.workdir {
            println!("   Working directory: {}", workdir);
        }
        println!("   Auto-restart: {}", task.auto_restart);

        Ok(())
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
        Commands::Logs { task, lines, log_type } => {
            task_manager.show_logs(&task, lines, &log_type);
        }
        Commands::Diagnose { task } => {
            task_manager.diagnose_task(&task)?;
        }
    }

    Ok(())
}
