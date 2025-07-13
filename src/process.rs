//! Process management for hyperV
//! 
//! Handles process spawning, monitoring, and termination with proper signal handling.

use crate::constants::SHUTDOWN_TIMEOUT;
use crate::error::{HyperVError, Result};
use crate::task::Task;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

/// Process manager for handling running tasks
pub struct ProcessManager {
    /// Currently running processes
    running_processes: HashMap<String, Child>,
}

impl ProcessManager {
    /// Create a new process manager
    pub fn new() -> Self {
        Self {
            running_processes: HashMap::new(),
        }
    }

    /// Check if a process with the given PID is running
    pub fn is_process_running(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            use libc::kill;
            unsafe { kill(pid as i32, 0) == 0 }
        }
        
        #[cfg(not(unix))]
        {
            // On non-Unix systems, we can't easily check if a process is running
            // This is a limitation for Windows support
            false
        }
    }

    /// Start a task process
    pub fn start_task(&mut self, task: &Task, task_env: &HashMap<String, String>, stdout_log: &Path, stderr_log: &Path) -> Result<u32> {
        // Validate the binary before starting
        self.validate_binary(&task.binary)?;

        // Create command
        let mut cmd = Command::new(&task.binary);
        cmd.args(&task.args);
        
        // Set environment variables
        for (key, value) in task_env {
            cmd.env(key, value);
        }
        
        // Set working directory
        if let Some(workdir) = &task.workdir {
            cmd.current_dir(workdir);
        }

        // Setup log files
        let stdout_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_log)
            .map_err(HyperVError::Io)?;
        
        let stderr_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_log)
            .map_err(HyperVError::Io)?;

        cmd.stdout(Stdio::from(stdout_file));
        cmd.stderr(Stdio::from(stderr_file));

        // Create process group for proper signal handling
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        // Spawn the process
        let child = cmd.spawn()
            .map_err(|e| HyperVError::ProcessStart(task.binary.clone(), e.to_string()))?;

        let pid = child.id();
        self.running_processes.insert(task.id.clone(), child);

        Ok(pid)
    }

    /// Stop a task process gracefully
    pub fn stop_task(&mut self, task_id: &str, pid: u32) -> Result<()> {
        // First check if the process is actually running
        if !self.is_process_running(pid) {
            println!("ℹ️  Process {} is already stopped", pid);
            // Remove from running processes if it's there
            self.running_processes.remove(task_id);
            return Ok(());
        }

        // First try graceful shutdown with SIGTERM
        #[cfg(unix)]
        {
            use libc::{kill, SIGTERM, SIGKILL};
            
            // Try to send SIGTERM to the process group first
            println!("🛑 Sending SIGTERM to process group {}", pid);
            let group_result = unsafe { kill(-(pid as i32), SIGTERM) };
            
            if group_result != 0 {
                // If process group signal failed, try signaling the individual process
                println!("⚠️  Process group signal failed, trying individual process...");
                let process_result = unsafe { kill(pid as i32, SIGTERM) };
                if process_result != 0 {
                    // Check if the process died between our checks
                    if !self.is_process_running(pid) {
                        println!("ℹ️  Process {} terminated during stop attempt", pid);
                        self.running_processes.remove(task_id);
                        return Ok(());
                    }
                    
                    // Get errno for better error reporting
                    let errno = std::io::Error::last_os_error();
                    return Err(HyperVError::ProcessStop(
                        format!("Failed to send SIGTERM to process {} (errno: {})", pid, errno)
                    ));
                }
            }
            
            println!("⏳ Waiting {} seconds for graceful shutdown...", SHUTDOWN_TIMEOUT.as_secs());
            
            // Wait for graceful shutdown
            thread::sleep(SHUTDOWN_TIMEOUT);
            
            // Check if process is still running
            if self.is_process_running(pid) {
                println!("💀 Process still running, sending SIGKILL...");
                
                // Try SIGKILL on process group first, then individual process
                let group_kill_result = unsafe { kill(-(pid as i32), SIGKILL) };
                if group_kill_result != 0 {
                    let process_kill_result = unsafe { kill(pid as i32, SIGKILL) };
                    if process_kill_result != 0 {
                        // Check if the process died during our attempts
                        if !self.is_process_running(pid) {
                            println!("ℹ️  Process {} terminated during kill attempt", pid);
                            self.running_processes.remove(task_id);
                            return Ok(());
                        }
                        
                        let errno = std::io::Error::last_os_error();
                        return Err(HyperVError::ProcessStop(
                            format!("Failed to kill process {} (errno: {})", pid, errno)
                        ));
                    }
                }
                thread::sleep(Duration::from_millis(500)); // Give it time to die
            }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, try to terminate the child process
            if let Some(mut child) = self.running_processes.remove(task_id) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }

        // Remove from running processes
        self.running_processes.remove(task_id);
        Ok(())
    }

    /// Validate that a binary file exists and is executable
    fn validate_binary(&self, binary_path: &str) -> Result<()> {
        let path = Path::new(binary_path);
        
        // Check if file exists
        if !path.exists() {
            return Err(HyperVError::BinaryNotFound(binary_path.to_string()));
        }

        // Check if file is executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path)
                .map_err(HyperVError::Io)?;
            let permissions = metadata.permissions();
            
            if permissions.mode() & 0o111 == 0 {
                return Err(HyperVError::BinaryNotExecutable(binary_path.to_string()));
            }
        }

        // Check for script files and validate shebang
        self.validate_script(path)?;

        Ok(())
    }

    /// Validate script files and check for proper shebang
    fn validate_script(&self, path: &Path) -> Result<()> {
        let mut file = std::fs::File::open(path)
            .map_err(HyperVError::Io)?;
        
        let mut buffer = [0; 512];
        let bytes_read = file.read(&mut buffer).unwrap_or(0);
        
        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 {
            // Has shebang - validate interpreter
            let shebang_content = String::from_utf8_lossy(&buffer[..bytes_read.min(256)]);
            let shebang_line = shebang_content
                .lines()
                .next()
                .unwrap_or("")
                .trim();
            
            if let Some(interpreter) = shebang_line.strip_prefix("#!") {
                let interpreter = interpreter.trim().split_whitespace().next().unwrap_or("");
                if !interpreter.is_empty() && !Path::new(interpreter).exists() {
                    return Err(HyperVError::InterpreterNotFound(interpreter.to_string()));
                }
            }
        } else if buffer.iter().take(bytes_read).all(|&b| b.is_ascii() && b != 0) {
            // Text file without shebang - warn but don't error
            eprintln!("⚠️  Warning: Text file without shebang detected: {}", path.display());
            eprintln!("💡 If this is a shell script, add '#!/bin/bash' as the first line");
        }

        Ok(())
    }

    /// Check if a task is currently managed by this process manager
    pub fn is_task_running(&self, task_id: &str) -> bool {
        self.running_processes.contains_key(task_id)
    }

    /// Get the number of running processes
    pub fn running_count(&self) -> usize {
        self.running_processes.len()
    }

    /// Clean up zombie processes and update their exit codes
    pub fn cleanup_zombies(&mut self) -> HashMap<String, i32> {
        let mut to_remove = Vec::new();
        let mut exit_codes = HashMap::new();
        
        for (task_id, child) in &mut self.running_processes {
            match child.try_wait() {
                Ok(Some(status)) => {
                    to_remove.push(task_id.clone());
                    if let Some(code) = status.code() {
                        exit_codes.insert(task_id.clone(), code);
                    }
                }
                Ok(None) => { /* Still running */ }
                Err(e) => {
                    eprintln!("Error waiting for child process {}: {}", task_id, e);
                    to_remove.push(task_id.clone()); // Remove to avoid repeated errors
                }
            }
        }
        
        for task_id in to_remove {
            self.running_processes.remove(&task_id);
        }
        exit_codes
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnose issues with a binary file
pub fn diagnose_binary(binary_path: &str) -> Result<()> {
    let path = Path::new(binary_path);
    
    println!("🔍 Diagnosing binary: {}", binary_path);
    println!();
    
    // Check file existence
    if !path.exists() {
        println!("❌ File does not exist");
        return Err(HyperVError::BinaryNotFound(binary_path.to_string()));
    }
    println!("✅ File exists");
    
    // Check file type
    let metadata = std::fs::metadata(path)
        .map_err(HyperVError::Io)?;
    
    if metadata.is_dir() {
        println!("❌ Path points to a directory, not a file");
        return Err(HyperVError::InvalidBinary("Path is a directory".to_string()));
    }
    println!("✅ Is a file");
    
    // Check permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = metadata.permissions();
        let mode = permissions.mode();
        
        println!("📋 File permissions: {:o}", mode & 0o777);
        
        if mode & 0o111 == 0 {
            println!("❌ File is not executable");
            println!("💡 Fix with: chmod +x {}", binary_path);
            return Err(HyperVError::BinaryNotExecutable(binary_path.to_string()));
        }
        println!("✅ File is executable");
    }
    
    // Analyze file content
    let mut file = std::fs::File::open(path)
        .map_err(HyperVError::Io)?;
    
    let mut buffer = [0; 512];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);
    
    if bytes_read == 0 {
        println!("❌ File is empty");
        return Err(HyperVError::InvalidBinary("File is empty".to_string()));
    }
    
    // Check for binary vs text
    let is_binary = buffer.iter().take(bytes_read).any(|&b| b == 0 || (!b.is_ascii() && b != b'\n' && b != b'\r' && b != b'\t'));
    
    if is_binary {
        println!("✅ Detected binary file");
        
        // Check for common binary formats
        if bytes_read >= 4 {
            match &buffer[0..4] {
                [0x7f, b'E', b'L', b'F'] => println!("📋 Format: ELF executable (Linux)"),
                [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] => println!("📋 Format: Mach-O executable (macOS)"),
                [b'M', b'Z', _, _] => println!("📋 Format: PE executable (Windows)"),
                _ => println!("📋 Format: Unknown binary format"),
            }
        }
    } else {
        println!("📋 Detected text file (script)");
        
        // Check for shebang
        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 {
            let shebang_content = String::from_utf8_lossy(&buffer[..bytes_read.min(256)]);
            let shebang_line = shebang_content
                .lines()
                .next()
                .unwrap_or("")
                .trim();
            
            println!("✅ Has shebang: {}", shebang_line);
            
            // Validate interpreter
            if let Some(interpreter) = shebang_line.strip_prefix("#!") {
                let interpreter = interpreter.trim().split_whitespace().next().unwrap_or("");
                if !interpreter.is_empty() {
                    if Path::new(interpreter).exists() {
                        println!("✅ Interpreter exists: {}", interpreter);
                    } else {
                        println!("❌ Interpreter not found: {}", interpreter);
                        println!("💡 Install the interpreter or fix the shebang line");
                        return Err(HyperVError::InterpreterNotFound(interpreter.to_string()));
                    }
                }
            }
        } else {
            println!("❌ No shebang found");
            println!("💡 Add a shebang line like '#!/bin/bash' as the first line");
        }
    }
    
    println!();
    println!("🎯 Diagnosis complete - binary appears valid");
    Ok(())
}
