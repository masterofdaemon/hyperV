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
        let mut compose: ComposeFile = serde_yml::from_str(&content)
            .map_err(|e| HyperVError::InvalidInput(format!("Failed to parse YAML: {}", e)))?;
        // Expand environment-variable references in env values so secrets
        // (e.g. SURREAL_PASSWORD: "${SURREAL_PASSWORD:-...}") come from the
        // process environment instead of being committed in plaintext.
        // Args/binary/workdir are left untouched: `$VAR` there is expanded
        // by the task's shell at runtime from the task env.
        for svc in compose.services.values_mut() {
            for value in svc.env.values_mut() {
                *value = expand_env_value(value);
            }
        }
        Ok(compose)
    }
}

/// Expand `$VAR`, `${VAR}`, `${VAR:-default}` and `${VAR-default}` from the
/// process environment. Unset variables without a default expand to "".
fn expand_env_value(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        // Handle `$$` escape and trailing `$`.
        if i + 1 >= bytes.len() {
            out.push('$');
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        if next == b'$' {
            out.push('$');
            i += 2;
            continue;
        }
        if next == b'{' {
            // Find closing brace.
            if let Some(end) = input[i + 2..].find('}') {
                let inner = &input[i + 2..i + 2 + end];
                // Split `${VAR:-default}` / `${VAR-default}`.
                let (name, default) = if let Some(pos) = inner.find(":-") {
                    (&inner[..pos], Some(&inner[pos + 2..]))
                } else if let Some(pos) = inner.find('-') {
                    // Only treat `-` as a default separator when it is a valid
                    // split point (avoids breaking exotic names).
                    (&inner[..pos], Some(&inner[pos + 1..]))
                } else {
                    (inner, None)
                };
                let value = std::env::var(name).ok().filter(|v| {
                    // `${VAR:-default}` falls back on unset OR empty.
                    !(inner.contains(":-") && v.is_empty())
                });
                match value {
                    Some(v) => out.push_str(&v),
                    None => {
                        if let Some(d) = default {
                            out.push_str(d);
                        }
                    }
                }
                i += 2 + end + 1;
                continue;
            }
            // No closing brace: keep literal.
            out.push('$');
            i += 1;
            continue;
        }
        // `$VAR` form: alphanumeric + underscore.
        let mut end = i + 1;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if end == i + 1 {
            // `$` not followed by a name: keep literal.
            out.push('$');
            i += 1;
            continue;
        }
        let name = &input[i + 1..end];
        if let Ok(v) = std::env::var(name) {
            out.push_str(&v);
        }
        i = end;
    }
    out
}

impl TaskManager {
    /// Apply services from a compose file: create or update tasks to match the file
    pub fn up_from_compose(&mut self, compose: &ComposeFile) -> Result<()> {
        let _lock_file = self.lock_tasks_for_update()?;
        // Create or update tasks for each service
        for (name, svc) in &compose.services {
            // Convert env map to vec of KEY=VALUE like CLI create expects
            let env_vars: Vec<String> = svc
                .env
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();

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
                    env_vars,
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
