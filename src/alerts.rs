//! Alerting support for severe task failures.
//!
//! Alert history and cooldowns are intentionally process-local. Restarting the
//! daemon clears alert state and starts a fresh detection window.

use crate::error::{HyperVError, Result};
use crate::task::{Task, TaskStatus};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::time::Duration as StdDuration;

const CRASH_LOOP_THRESHOLD: usize = 2;
const CRASH_LOOP_WINDOW_MINUTES: i64 = 10;
const ALERT_COOLDOWN_MINUTES: i64 = 30;
const TELEGRAM_TIMEOUT_SECONDS: u64 = 5;
const TELEGRAM_DISABLE_WEB_PAGE_PREVIEW: &str = "true";
const TELEGRAM_ERROR_BODY_LIMIT_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertReason {
    CrashLoop { crashes: usize, window_minutes: i64 },
    RestartExhausted { attempts: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub task_id: String,
    pub task_name: String,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub reason: AlertReason,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct AlertTracker {
    crashes_by_task: HashMap<String, Vec<DateTime<Utc>>>,
    sent_at_by_key: HashMap<String, DateTime<Utc>>,
    restart_exhausted_by_task: HashMap<String, u32>,
}

impl AlertTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_crash(&mut self, task: &Task, now: DateTime<Utc>) -> Option<Alert> {
        let window_start = now - Duration::minutes(CRASH_LOOP_WINDOW_MINUTES);
        let crashes = self.crashes_by_task.entry(task.id.clone()).or_default();
        crashes.retain(|crash_at| *crash_at >= window_start);
        crashes.push(now);

        let crash_count = crashes.len();
        if crash_count < CRASH_LOOP_THRESHOLD {
            return None;
        }

        let cooldown_key = format!("crash-loop:{}", task.id);
        if self.cooldown_active(&cooldown_key, now) {
            return None;
        }
        self.sent_at_by_key.insert(cooldown_key, now);

        Some(Alert {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            restart_count: task.restart_count,
            last_exit_code: task.last_exit_code,
            reason: AlertReason::CrashLoop {
                crashes: crash_count,
                window_minutes: CRASH_LOOP_WINDOW_MINUTES,
            },
            detected_at: now,
        })
    }

    pub fn check_restart_exhausted(
        &mut self,
        task: &Task,
        max_attempts: u32,
        now: DateTime<Utc>,
    ) -> Option<Alert> {
        if task.restart_count < max_attempts || task.status != TaskStatus::Failed {
            self.restart_exhausted_by_task.remove(&task.id);
            return None;
        }

        if self
            .restart_exhausted_by_task
            .get(&task.id)
            .is_some_and(|alerted_count| *alerted_count == task.restart_count)
        {
            return None;
        }

        let cooldown_key = format!("restart-exhausted:{}", task.id);
        if self.cooldown_active(&cooldown_key, now) {
            return None;
        }
        self.sent_at_by_key.insert(cooldown_key, now);
        self.restart_exhausted_by_task
            .insert(task.id.clone(), task.restart_count);

        Some(Alert {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            restart_count: task.restart_count,
            last_exit_code: task.last_exit_code,
            reason: AlertReason::RestartExhausted {
                attempts: task.restart_count,
            },
            detected_at: now,
        })
    }

    fn cooldown_active(&self, key: &str, now: DateTime<Utc>) -> bool {
        self.sent_at_by_key
            .get(key)
            .is_some_and(|sent_at| now - *sent_at < Duration::minutes(ALERT_COOLDOWN_MINUTES))
    }
}

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

impl TelegramConfig {
    pub fn from_env() -> Option<Self> {
        let bot_token = std::env::var("HYPERV_TELEGRAM_BOT_TOKEN").ok()?;
        let chat_id = std::env::var("HYPERV_TELEGRAM_CHAT_ID").ok()?;

        if bot_token.trim().is_empty() || chat_id.trim().is_empty() {
            return None;
        }

        Some(Self { bot_token, chat_id })
    }
}

#[derive(Debug, Clone)]
pub struct TelegramNotifier {
    config: TelegramConfig,
    agent: ureq::Agent,
}

impl TelegramNotifier {
    pub fn from_env() -> Option<Self> {
        TelegramConfig::from_env().map(Self::new)
    }

    pub fn new(config: TelegramConfig) -> Self {
        let timeout = StdDuration::from_secs(TELEGRAM_TIMEOUT_SECONDS);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .timeout_write(timeout)
            .build();

        Self { config, agent }
    }

    pub fn send_alert(&self, alert: &Alert) -> Result<()> {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.config.bot_token
        );
        let text = format_alert_message(alert);

        self.agent
            .post(&url)
            .send_form(&[
                ("chat_id", self.config.chat_id.as_str()),
                ("text", text.as_str()),
                (
                    "disable_web_page_preview",
                    TELEGRAM_DISABLE_WEB_PAGE_PREVIEW,
                ),
            ])
            .map_err(|err| {
                HyperVError::LogError(format!(
                    "Telegram alert failed: {}",
                    sanitize_delivery_error(err, &self.config.bot_token)
                ))
            })?;

        Ok(())
    }
}

fn sanitize_delivery_error(err: ureq::Error, bot_token: &str) -> String {
    match err {
        ureq::Error::Status(status, response) => {
            let body = sanitize_response_body(response, bot_token);
            if body.is_empty() {
                format!("Telegram API returned HTTP status {status}")
            } else {
                format!("Telegram API returned HTTP status {status}: {body}")
            }
        }
        ureq::Error::Transport(err) => sanitize_error_message(&err.to_string(), bot_token),
    }
}

fn sanitize_response_body(response: ureq::Response, bot_token: &str) -> String {
    response
        .into_string()
        .map(|body| truncate_error_body(&sanitize_error_message(body.trim(), bot_token)))
        .unwrap_or_else(|err| {
            format!(
                "failed to read response body: {}",
                sanitize_error_message(&err.to_string(), bot_token)
            )
        })
}

fn sanitize_error_message(message: &str, bot_token: &str) -> String {
    if bot_token.is_empty() {
        message.to_string()
    } else {
        message.replace(bot_token, "[REDACTED]")
    }
}

fn truncate_error_body(body: &str) -> String {
    if body.chars().count() > TELEGRAM_ERROR_BODY_LIMIT_CHARS {
        format!(
            "{}...",
            body.chars()
                .take(TELEGRAM_ERROR_BODY_LIMIT_CHARS)
                .collect::<String>()
        )
    } else {
        body.to_string()
    }
}

pub fn format_alert_message(alert: &Alert) -> String {
    let reason = match alert.reason {
        AlertReason::CrashLoop {
            crashes,
            window_minutes,
        } => format!("crash loop: {crashes} crashes in {window_minutes} minutes"),
        AlertReason::RestartExhausted { attempts } => {
            format!("restart attempts exhausted: {attempts} attempts")
        }
    };
    let exit_code = alert
        .last_exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "hyperV alert\nTask: {}\nReason: {}\nRestart count: {}\nLast exit code: {}\nDetected at: {}",
        alert.task_name,
        reason,
        alert.restart_count,
        exit_code,
        alert.detected_at.to_rfc3339()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_telegram_token_from_delivery_errors() {
        let token = "123456:secret-token";
        let message = format!("failed to reach https://api.telegram.org/bot{token}/sendMessage");

        let sanitized = sanitize_error_message(&message, token);

        assert!(!sanitized.contains(token));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn includes_sanitized_telegram_status_body() {
        let token = "123456:secret-token";
        let response = ureq::Response::new(
            401,
            "Unauthorized",
            &format!(r#"{{"description":"bad token {token}"}}"#),
        )
        .expect("test response");

        let sanitized = sanitize_delivery_error(ureq::Error::Status(401, response), token);

        assert!(sanitized.contains("HTTP status 401"));
        assert!(sanitized.contains("bad token [REDACTED]"));
        assert!(!sanitized.contains(token));
    }
}
