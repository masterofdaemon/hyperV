//! Process management for hyperV
//!
//! Handles process spawning, monitoring, and termination with proper signal handling.

use crate::constants::SHUTDOWN_TIMEOUT;
use crate::error::{HyperVError, Result};
use crate::task::Task;
use std::collections::HashMap;
#[cfg(unix)]
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

    fn is_pid_running(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // kill(pid, 0) doesn't send a signal; it only performs error checking.
            // - ESRCH: no such process
            // - EPERM: process exists but we don't have permission (treat as running)
            use libc::kill;
            let rc = unsafe { kill(pid as i32, 0) };
            if rc == 0 {
                return true;
            }
            let err = std::io::Error::last_os_error();
            !matches!(err.raw_os_error(), Some(libc::ESRCH))
        }

        #[cfg(not(unix))]
        {
            use sysinfo::{Pid, System};
            let mut system = System::new();
            let pid = Pid::from_u32(pid);
            system.refresh_process(pid)
        }
    }

    fn is_pgid_running(pgid: u32) -> bool {
        #[cfg(unix)]
        {
            use libc::kill;
            let rc = unsafe { kill(-(pgid as i32), 0) };
            if rc == 0 {
                return true;
            }
            let err = std::io::Error::last_os_error();
            !matches!(err.raw_os_error(), Some(libc::ESRCH))
        }

        #[cfg(not(unix))]
        {
            let _ = pgid;
            false
        }
    }

    #[cfg(unix)]
    fn process_group_id(pid: u32) -> Option<u32> {
        let pgid = unsafe { libc::getpgid(pid as i32) };
        if pgid > 0 { Some(pgid as u32) } else { None }
    }

    #[cfg(unix)]
    fn descendant_pids(root_pid: u32) -> Vec<u32> {
        use sysinfo::{Pid, System};

        let mut system = System::new();
        system.refresh_processes();

        let mut descendants = Vec::new();
        let mut stack = vec![Pid::from_u32(root_pid)];

        while let Some(parent_pid) = stack.pop() {
            for (pid, process) in system.processes() {
                if process.parent() == Some(parent_pid) {
                    descendants.push(pid.as_u32());
                    stack.push(*pid);
                }
            }
        }

        descendants
    }

    /// Check if a process with the given PID is running
    pub fn is_process_running(&self, pid: u32) -> bool {
        Self::is_pid_running(pid)
    }

    /// Check if a process group with the given PGID (usually the task's initial PID) is running.
    pub fn is_process_group_running(&self, pgid: u32) -> bool {
        Self::is_pgid_running(pgid)
    }

    /// Best-effort process start time used to detect PID reuse.
    pub fn process_start_time(&self, pid: u32) -> Option<u64> {
        use sysinfo::{Pid, System};
        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        if !system.refresh_process(pid) {
            return None;
        }
        system.process(pid).map(|p| p.start_time())
    }

    /// Best-effort process exe path for identity checks.
    pub fn process_exe(&self, pid: u32) -> Option<std::path::PathBuf> {
        use sysinfo::{Pid, System};
        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        if !system.refresh_process(pid) {
            return None;
        }
        system
            .process(pid)
            .and_then(|p| p.exe().map(|p| p.to_path_buf()))
    }

    /// Best-effort process command line for identity checks (useful for scripts launched via an interpreter).
    pub fn process_cmd(&self, pid: u32) -> Option<Vec<String>> {
        use sysinfo::{Pid, System};
        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        if !system.refresh_process(pid) {
            return None;
        }
        system.process(pid).map(|p| p.cmd().to_vec())
    }

    /// Return true if the current process at `pid` appears to match the expected identity.
    /// This is used to reduce the risk of killing an unrelated process after PID reuse.
    pub fn pid_matches_identity(
        &self,
        pid: u32,
        binary: &str,
        pid_start_time: Option<u64>,
    ) -> bool {
        // Prefer start_time: it's the strongest signal against PID reuse.
        if let Some(expected) = pid_start_time {
            return self
                .process_start_time(pid)
                .is_some_and(|actual| actual == expected);
        }

        // Fallback to exe path when we don't have a start_time snapshot.
        if binary.is_empty() {
            return true;
        }

        let expected = std::fs::canonicalize(binary).unwrap_or_else(|_| binary.into());

        // Prefer cmdline matching: scripts often show up as the interpreter in `exe()`,
        // but the script path still appears in `cmd()`.
        if let Some(cmd) = self.process_cmd(pid) {
            for arg in cmd {
                if arg == binary {
                    return true;
                }
                let arg_path = std::path::PathBuf::from(&arg);
                if let Ok(arg_canon) = std::fs::canonicalize(&arg_path)
                    && arg_canon == expected
                {
                    return true;
                }
            }
        }

        // Fallback: exe path equality for normal binaries.
        let actual = self
            .process_exe(pid)
            .and_then(|p| std::fs::canonicalize(&p).ok().or(Some(p)));
        actual.is_some_and(|p| p == expected)
    }

    /// Start a task process
    pub fn start_task(
        &mut self,
        task: &Task,
        task_env: &HashMap<String, String>,
        stdout_log: &Path,
        stderr_log: &Path,
    ) -> Result<u32> {
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
        let child = cmd
            .spawn()
            .map_err(|e| HyperVError::ProcessStart(task.binary.clone(), e.to_string()))?;

        let pid = child.id();
        self.running_processes.insert(task.id.clone(), child);

        Ok(pid)
    }

    /// Stop a task process gracefully
    pub fn stop_task(&mut self, task_id: &str, pid: u32) -> Result<()> {
        // Take ownership of the Child so we can poll/reap without borrowing self.
        // If the process doesn't actually terminate, we reinsert it.
        let mut child = self.running_processes.remove(task_id);

        let mut wait_for_exit = |timeout: Duration| -> bool {
            let start = Instant::now();
            while start.elapsed() < timeout {
                if let Some(c) = child.as_mut() {
                    // try_wait() reaps the child if it exited.
                    let exited = matches!(c.try_wait(), Ok(Some(_)));
                    if exited {
                        child.take();
                        return true;
                    }
                }

                if !Self::is_pid_running(pid) && !Self::is_pgid_running(pid) {
                    // If the OS no longer reports it running, best-effort reap to avoid zombies.
                    if let Some(mut c) = child.take() {
                        let _ = c.try_wait();
                        let _ = c.wait();
                    }
                    return true;
                }

                thread::sleep(Duration::from_millis(100));
            }

            // One final check at the boundary.
            if let Some(c) = child.as_mut() {
                let exited = matches!(c.try_wait(), Ok(Some(_)));
                if exited {
                    child.take();
                    return true;
                }
            }
            !Self::is_pid_running(pid) && !Self::is_pgid_running(pid)
        };

        // First check if the process is actually running
        if !Self::is_pid_running(pid) && !Self::is_pgid_running(pid) {
            println!("ℹ️  Process {} is already stopped", pid);
            // Best-effort reap any tracked child to avoid zombies.
            if let Some(mut c) = child.take() {
                let _ = c.try_wait();
                let _ = c.wait();
            }
            return Ok(());
        }

        // First try graceful shutdown with SIGTERM
        #[cfg(unix)]
        {
            use libc::{SIGKILL, SIGTERM, kill};

            let descendant_pids = Self::descendant_pids(pid);
            let mut watched_pids = descendant_pids.clone();
            watched_pids.push(pid);

            let mut watched_pgids = HashSet::new();
            watched_pgids.insert(pid);
            for child_pid in &descendant_pids {
                if let Some(pgid) = Self::process_group_id(*child_pid) {
                    watched_pgids.insert(pgid);
                }
            }

            let all_stopped = |watched_pids: &[u32], watched_pgids: &HashSet<u32>| {
                !watched_pids.iter().any(|pid| Self::is_pid_running(*pid))
                    && !watched_pgids
                        .iter()
                        .any(|pgid| Self::is_pgid_running(*pgid))
            };

            let send_signal = |signal| {
                let mut sent = false;
                for pgid in &watched_pgids {
                    if unsafe { kill(-(*pgid as i32), signal) } == 0 {
                        sent = true;
                    }
                }
                for watched_pid in &watched_pids {
                    if Self::is_pid_running(*watched_pid)
                        && unsafe { kill(*watched_pid as i32, signal) } == 0
                    {
                        sent = true;
                    }
                }
                sent
            };

            // Try to send SIGTERM to the process group first
            println!("🛑 Sending SIGTERM to process group {}", pid);
            if !send_signal(SIGTERM) {
                // Check if the process died between our checks
                if all_stopped(&watched_pids, &watched_pgids) {
                    println!("ℹ️  Process {} terminated during stop attempt", pid);
                    if let Some(mut c) = child.take() {
                        let _ = c.try_wait();
                        let _ = c.wait();
                    }
                    return Ok(());
                }

                let errno = std::io::Error::last_os_error();
                return Err(HyperVError::ProcessStop(format!(
                    "Failed to send SIGTERM to process {} or its children (errno: {})",
                    pid, errno
                )));
            }

            println!(
                "⏳ Waiting {} seconds for graceful shutdown...",
                SHUTDOWN_TIMEOUT.as_secs()
            );

            if !wait_for_exit(SHUTDOWN_TIMEOUT) {
                println!("💀 Process still running, sending SIGKILL...");

                if !send_signal(SIGKILL) {
                    // Check if the process died during our attempts
                    if all_stopped(&watched_pids, &watched_pgids) {
                        println!("ℹ️  Process {} terminated during kill attempt", pid);
                        if let Some(mut c) = child.take() {
                            let _ = c.try_wait();
                            let _ = c.wait();
                        }
                        return Ok(());
                    }

                    let errno = std::io::Error::last_os_error();
                    if let Some(c) = child.take() {
                        self.running_processes.insert(task_id.to_string(), c);
                    }
                    return Err(HyperVError::ProcessStop(format!(
                        "Failed to kill process {} or its children (errno: {})",
                        pid, errno
                    )));
                }

                // Give it a chance to actually terminate after SIGKILL.
                let kill_timeout = Duration::from_secs(2);
                if !wait_for_exit(kill_timeout) || !all_stopped(&watched_pids, &watched_pgids) {
                    if let Some(c) = child.take() {
                        self.running_processes.insert(task_id.to_string(), c);
                    }
                    return Err(HyperVError::ProcessStop(format!(
                        "Process {} or one of its children did not terminate after SIGKILL",
                        pid
                    )));
                }
            }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix systems, try to terminate the child process
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            } else if Self::is_pid_running(pid) {
                return Err(HyperVError::ProcessStop(format!(
                    "Process {} is still running but is not managed by this daemon",
                    pid
                )));
            }
        }

        // If we still have a tracked child here, ensure it's reaped before dropping.
        if let Some(mut c) = child.take() {
            let _ = c.try_wait();
            let _ = c.wait();
        }

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
            let metadata = std::fs::metadata(path).map_err(HyperVError::Io)?;
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
        let mut file = std::fs::File::open(path).map_err(HyperVError::Io)?;

        let mut buffer = [0; 512];
        let bytes_read = file.read(&mut buffer).unwrap_or(0);

        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 {
            // Has shebang - validate interpreter
            let shebang_content = String::from_utf8_lossy(&buffer[..bytes_read.min(256)]);
            let shebang_line = shebang_content.lines().next().unwrap_or("").trim();

            if let Some(interpreter) = shebang_line.strip_prefix("#!") {
                let interpreter = interpreter.split_whitespace().next().unwrap_or("");
                if !interpreter.is_empty() && !Path::new(interpreter).exists() {
                    return Err(HyperVError::InterpreterNotFound(interpreter.to_string()));
                }
            }
        } else if buffer
            .iter()
            .take(bytes_read)
            .all(|&b| b.is_ascii() && b != 0)
        {
            // Text file without shebang - warn but don't error
            eprintln!(
                "⚠️  Warning: Text file without shebang detected: {}",
                path.display()
            );
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
    let metadata = std::fs::metadata(path).map_err(HyperVError::Io)?;

    if metadata.is_dir() {
        println!("❌ Path points to a directory, not a file");
        return Err(HyperVError::InvalidBinary(
            "Path is a directory".to_string(),
        ));
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
    let mut file = std::fs::File::open(path).map_err(HyperVError::Io)?;

    let mut buffer = [0; 512];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);

    if bytes_read == 0 {
        println!("❌ File is empty");
        return Err(HyperVError::InvalidBinary("File is empty".to_string()));
    }

    // Check for binary vs text
    let is_binary = buffer
        .iter()
        .take(bytes_read)
        .any(|&b| b == 0 || (!b.is_ascii() && b != b'\n' && b != b'\r' && b != b'\t'));

    if is_binary {
        println!("✅ Detected binary file");

        // Check for common binary formats
        if bytes_read >= 4 {
            match &buffer[0..4] {
                [0x7f, b'E', b'L', b'F'] => println!("📋 Format: ELF executable (Linux)"),
                [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] => {
                    println!("📋 Format: Mach-O executable (macOS)")
                }
                [b'M', b'Z', _, _] => println!("📋 Format: PE executable (Windows)"),
                _ => println!("📋 Format: Unknown binary format"),
            }
        }
    } else {
        println!("📋 Detected text file (script)");

        // Check for shebang
        if bytes_read >= 2 && buffer[0] == 0x23 && buffer[1] == 0x21 {
            let shebang_content = String::from_utf8_lossy(&buffer[..bytes_read.min(256)]);
            let shebang_line = shebang_content.lines().next().unwrap_or("").trim();

            println!("✅ Has shebang: {}", shebang_line);

            // Validate interpreter
            if let Some(interpreter) = shebang_line.strip_prefix("#!") {
                let interpreter = interpreter.split_whitespace().next().unwrap_or("");
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
