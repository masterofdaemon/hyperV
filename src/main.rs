//! hyperV Service Manager
//! 
//! A command-line application for running and managing binary services
//! on Linux and macOS with advanced process management and monitoring.

use clap::Parser;
use hyperV::{cli::{Cli, Commands}, manager::TaskManager, Result};

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
            run_daemon_mode(task_manager).await?;
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
