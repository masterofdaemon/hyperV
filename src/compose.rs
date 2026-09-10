use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{HyperVError, Result};
use crate::manager::TaskManager;

#[derive(Debug, Deserialize)]
pub struct ComposeFile {
    pub services: HashMap<String, Service>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Service {
    pub binary: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub auto_restart: bool,
}

impl ComposeFile {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).map_err(HyperVError::Io)?;
        let compose: ComposeFile = serde_yml::from_str(&content)
            .map_err(|e| HyperVError::InvalidInput(format!("Failed to parse YAML: {}", e)))?;
        Ok(compose)
    }
}

impl TaskManager {
    /// Apply services from a compose file: create or update tasks to match the file
    pub fn up_from_compose(&mut self, compose: &ComposeFile) -> Result<()> {
        self.with_tasks_lock(|s| s.up_from_compose_locked(compose))
    }

    fn up_from_compose_locked(&mut self, compose: &ComposeFile) -> Result<()> {
        // Create or update tasks for each service
        for (name, svc) in &compose.services {
            // Convert env map to vec of KEY=VALUE like CLI create expects
            let env_vars: Vec<String> = svc
                .env
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();

            // If task exists, replace its configuration; otherwise create
            if let Some(task) = self.find_task_mut(name) {
                task.binary = svc.binary.clone();
                task.args = svc.args.clone();
                task.env = svc.env.clone();
                task.workdir = svc.workdir.clone();
                task.auto_restart = svc.auto_restart;
            } else {
                self.create_task(
                    name.clone(),
                    svc.binary.clone(),
                    svc.args.clone(),
                    env_vars,
                    svc.workdir.clone(),
                    svc.auto_restart,
                )?;
            }
        }

        // Remove tasks that are not in the compose file? For safety, we won't automatically remove.
        // Users can run `down` to remove only compose-defined tasks.
        self.save()?;
        Ok(())
    }

    /// Remove tasks that are defined in the compose file
    pub fn down_from_compose(&mut self, compose: &ComposeFile) -> Result<()> {
        self.with_tasks_lock(|s| s.down_from_compose_locked(compose))
    }

    fn down_from_compose_locked(&mut self, compose: &ComposeFile) -> Result<()> {
        // Tear every service down before reporting, so one stubborn task does not strand the
        // rest, but do report: silently swallowing these left `down` claiming success while
        // tasks were still configured and running.
        let mut failed: Vec<&str> = Vec::new();

        for name in compose.services.keys() {
            if self.find_task(name).is_none() {
                continue;
            }
            // remove_task stops the task first if it is still running.
            if let Err(e) = self.remove_task(name) {
                eprintln!("⚠️  Failed to remove task \"{}\": {}", name, e);
                failed.push(name);
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(HyperVError::ProcessStop(format!(
                "could not remove {} of {} service(s): {}",
                failed.len(),
                compose.services.len(),
                failed.join(", ")
            )))
        }
    }
}
