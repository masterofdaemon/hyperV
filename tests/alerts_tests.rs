use chrono::{Duration, TimeZone, Utc};
use hyperV::alerts::{AlertReason, AlertTracker};
use hyperV::constants::MAX_RESTART_ATTEMPTS;
use hyperV::{Task, TaskStatus};
use std::collections::HashMap;

fn task_with_state(id: &str, name: &str, restart_count: u32, exit_code: Option<i32>) -> Task {
    let mut task = Task::new(
        id.to_string(),
        name.to_string(),
        "/bin/false".to_string(),
        Vec::new(),
        HashMap::new(),
        None,
        true,
        None,
        None,
    );
    task.set_status(TaskStatus::Failed);
    task.restart_count = restart_count;
    task.last_exit_code = exit_code;
    task
}

#[test]
fn alerts_on_second_crash_inside_window() {
    let mut tracker = AlertTracker::new();
    let task = task_with_state("task-1", "api", 1, Some(137));
    let now = Utc.with_ymd_and_hms(2026, 5, 21, 10, 0, 0).unwrap();

    assert!(tracker.record_crash(&task, now).is_none());
    let alert = tracker
        .record_crash(&task, now + Duration::minutes(4))
        .expect("second crash should alert");

    assert_eq!(alert.task_id, "task-1");
    assert_eq!(alert.task_name, "api");
    assert_eq!(alert.restart_count, 1);
    assert_eq!(alert.last_exit_code, Some(137));
    assert_eq!(
        alert.reason,
        AlertReason::CrashLoop {
            crashes: 2,
            window_minutes: 10,
        }
    );
}

#[test]
fn does_not_alert_when_crashes_are_outside_window() {
    let mut tracker = AlertTracker::new();
    let task = task_with_state("task-1", "api", 1, None);
    let now = Utc.with_ymd_and_hms(2026, 5, 21, 10, 0, 0).unwrap();

    assert!(tracker.record_crash(&task, now).is_none());
    assert!(
        tracker
            .record_crash(&task, now + Duration::minutes(11))
            .is_none()
    );
}

#[test]
fn suppresses_crash_loop_alerts_until_cooldown_expires() {
    let mut tracker = AlertTracker::new();
    let task = task_with_state("task-1", "api", 1, None);
    let now = Utc.with_ymd_and_hms(2026, 5, 21, 10, 0, 0).unwrap();

    assert!(tracker.record_crash(&task, now).is_none());
    assert!(
        tracker
            .record_crash(&task, now + Duration::minutes(1))
            .is_some()
    );
    assert!(
        tracker
            .record_crash(&task, now + Duration::minutes(2))
            .is_none()
    );
    assert!(
        tracker
            .record_crash(&task, now + Duration::minutes(31))
            .is_none()
    );
    assert!(
        tracker
            .record_crash(&task, now + Duration::minutes(32))
            .is_some()
    );
}

#[test]
fn alerts_when_restart_attempts_are_exhausted() {
    let mut tracker = AlertTracker::new();
    let task = task_with_state("task-1", "api", MAX_RESTART_ATTEMPTS, Some(1));
    let now = Utc.with_ymd_and_hms(2026, 5, 21, 10, 0, 0).unwrap();

    let alert = tracker
        .check_restart_exhausted(&task, MAX_RESTART_ATTEMPTS, now)
        .expect("exhausted restart attempts should alert");

    assert_eq!(
        alert.reason,
        AlertReason::RestartExhausted {
            attempts: MAX_RESTART_ATTEMPTS
        }
    );
    assert!(
        tracker
            .check_restart_exhausted(&task, MAX_RESTART_ATTEMPTS, now + Duration::minutes(1))
            .is_none()
    );
    assert!(
        tracker
            .check_restart_exhausted(&task, MAX_RESTART_ATTEMPTS, now + Duration::minutes(31))
            .is_none()
    );

    let task_after_new_attempt =
        task_with_state("task-1", "api", MAX_RESTART_ATTEMPTS + 1, Some(1));
    assert!(
        tracker
            .check_restart_exhausted(
                &task_after_new_attempt,
                MAX_RESTART_ATTEMPTS,
                now + Duration::minutes(31),
            )
            .is_some()
    );
}
