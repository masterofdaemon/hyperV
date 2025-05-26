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
    }

    Ok(())
}
