//! hyperV Service Manager
//! 
//! A command-line application for running and managing binary services
//! on Linux and macOS with advanced process management and monitoring.

use clap::Parser;
use hyperV::{cli::{Cli, Commands}, manager::TaskManager, Result};
use hyperV::compose::ComposeFile;
use std::fs;
use std::process::{Command, Stdio};
use hyperV::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
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
            maybe_spawn_daemon(&mut task_manager)?;
        }
        Commands::Restart { task } => {
            task_manager.restart_task(&task)?;
            maybe_spawn_daemon(&mut task_manager)?;
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
        Commands::Logs { task, lines, log_type, follow } => {
            task_manager.show_logs(&task, lines, log_type, follow)?;
        }
        Commands::Diagnose { task } => {
            task_manager.diagnose_task(&task)?;
        }
        Commands::Daemon => {
            // Run in daemon mode - monitoring and auto-restarting tasks
            // We keep the file handle to maintain the lock
            let _lock_file = write_daemon_pid()?;
            let result = run_daemon_mode(task_manager).await;
            // On exit, remove pid file
            let _ = remove_daemon_pid();
            result?;
        }
        Commands::Up { file, start } => {
            let compose = ComposeFile::from_path(&file)?;
            task_manager.up_from_compose(&compose)?;
            if start {
                for name in compose.services.keys() {
                    let _ = task_manager.start_task(name);
                }
            }
            maybe_spawn_daemon(&mut task_manager)?;
            println!("✅ Applied services from {}", file);
        }
        Commands::Down { file } => {
            let compose = ComposeFile::from_path(&file)?;
            task_manager.down_from_compose(&compose)?;
            println!("✅ Removed services from {}", file);
        }
    }

    Ok(())
}

async fn run_daemon_mode(mut task_manager: TaskManager) -> Result<()> {
    use hyperV::constants::MAIN_LOOP_INTERVAL;
    use tokio::time::sleep;
    use tokio::signal;
    
    println!("🚀 Starting hyperV daemon mode...");
    println!("📋 Monitoring {} tasks ({} with auto-restart)", 
        task_manager.task_count(), 
        task_manager.tasks_with_autorestart_count());
    println!("💡 Use 'hyperV list' to check task status");
    println!("🛑 Press Ctrl+C to stop daemon");

    // Set up signal handler for graceful shutdown
    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("\n🛑 Received shutdown signal, stopping daemon...");
                break;
            }
            _ = sleep(MAIN_LOOP_INTERVAL) => {
                if let Err(e) = task_manager.cleanup() {
                    eprintln!("Error during cleanup: {}", e);
                }
                if let Err(e) = task_manager.check_and_restart_tasks() {
                    eprintln!("Error during task restart check: {}", e);
                }
            }
        }
    }

    println!("✅ Daemon stopped gracefully");
    Ok(())
}

fn write_daemon_pid() -> Result<std::fs::File> {
    use fs2::FileExt;
    let pid_path = Config::new()?.daemon_pid_path();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&pid_path)
        .map_err(hyperV::HyperVError::Io)?;
    
    // Try to lock the file explicitly
    file.try_lock_exclusive().map_err(|e| hyperV::HyperVError::TaskAlreadyRunning(format!("Daemon is already running or could not lock PID file: {}", e)))?;
    
    let pid = std::process::id();
    use std::io::Write;
    let mut file_write = &file; 
    write!(file_write, "{}", pid).map_err(hyperV::HyperVError::Io)?;
    
    // Return the file to keep the lock alive
    Ok(file)
}

fn remove_daemon_pid() -> Result<()> {
    let config = Config::new()?;
    let pid_path = config.daemon_pid_path();
    if pid_path.exists() { let _ = fs::remove_file(pid_path); }
    Ok(())
}

fn is_daemon_running() -> bool {
    use fs2::FileExt;
    if let Ok(config) = Config::new() {
        let pid_path = config.daemon_pid_path();
        if !pid_path.exists() {
            return false;
        }
        
        // Try to open and lock. If we CAN lock it, then no other daemon is holding it.
        // If we can't lock it, someone else is running.
        if let Ok(file) = std::fs::File::open(&pid_path) {
            if file.try_lock_exclusive().is_ok() {
                // We got the lock, so no one else is holding it -> not running (stale pid file?)
                return false;
            } else {
                // Could not lock -> someone running
                return true;
            }
        }
        // If file exists but we can't open it? Assume running or permission error.
        false
    } else {
        false
    }
}

fn maybe_spawn_daemon(task_manager: &mut TaskManager) -> Result<()> {
    if task_manager.any_autorestart_enabled() && !is_daemon_running() {
        // Spawn a background daemon
        if let Ok(current_exe) = std::env::current_exe() {
            let _child = Command::new(current_exe)
                .arg("daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| hyperV::HyperVError::ProcessStart("daemon".into(), e.to_string()))?;
        }
    }
    Ok(())
}
