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
        let _lock_file = self.lock_tasks_for_update()?;
        // Create or update tasks for each service
        for (name, svc) in &compose.services {
            // If task exists, replace its configuration; otherwise create
            if self
                .find_task(name)?
                .is_some_and(|task| task.status == crate::task::TaskStatus::Running)
            {
                self.stop_task_unlocked(name)?;
            }
            if let Some(task) = self.find_task_mut(name)? {
                task.binary = svc.binary.clone();
                task.args = svc.args.clone();
                task.env = svc.env.clone();
                task.workdir = svc.workdir.clone();
                task.auto_restart = svc.auto_restart;
            } else {
                self.create_task_unlocked(
                    name.clone(),
                    svc.binary.clone(),
                    svc.args.clone(),
                    svc.env.clone(),
                    svc.workdir.clone(),
                    svc.auto_restart,
                )?;
            }
        }

        // Remove tasks that are not in the compose file? For safety, we won't automatically remove.
        // Users can run `down` to remove only compose-defined tasks.
        self.save_unlocked()?;
        Ok(())
    }

    /// Remove tasks that are defined in the compose file
    pub fn down_from_compose(&mut self, compose: &ComposeFile) -> Result<()> {
        let _lock_file = self.lock_tasks_for_update()?;
        let mut errors = Vec::new();
        let names: Vec<String> = compose.services.keys().cloned().collect();
        for name in names {
            match self.remove_task_unlocked(&name) {
                Ok(()) | Err(HyperVError::TaskNotFound(_)) => {}
                Err(error) => errors.push(format!("{name}: {error}")),
            }
        }
        if !errors.is_empty() {
            return Err(HyperVError::ProcessError(errors.join("; ")));
        }
        Ok(())
    }
}
