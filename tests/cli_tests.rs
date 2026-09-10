#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::zombie_processes,
    deprecated
)]

use assert_cmd::Command;
use predicates::prelude::*;

use std::time::{Duration, Instant};
use tempfile::TempDir;

fn hyperv_cmd(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("hyperV").unwrap();
    cmd.env("HYPERV_CONFIG_DIR", temp_dir.path());
    cmd
}

fn abs_repo_path(rel: &str) -> String {
    std::env::current_dir()
        .expect("cwd")
        .join(rel)
        .to_string_lossy()
        .to_string()
}

fn bin_path(primary: &'static str, fallback: &'static str) -> &'static str {
    if std::path::Path::new(primary).exists() {
        primary
    } else {
        fallback
    }
}

/// Best-effort `stop` guard so a mid-test panic still cleans up the spawned
/// long-running process. Same Drop-based pattern as the `Reaper` in
/// `test_daemon_locking`.
struct StopGuard {
    config_dir: std::path::PathBuf,
    task: &'static str,
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        let bin = assert_cmd::cargo::cargo_bin("hyperV");
        let _ = std::process::Command::new(&bin)
            .args(["stop", self.task])
            .env("HYPERV_CONFIG_DIR", &self.config_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[test]
fn test_help() {
    let temp = TempDir::new().unwrap();
    hyperv_cmd(&temp)
        .arg("--help")
        .assert()
        .success()
        // Check for the "about" text configured in Clap
        .stdout(predicate::str::contains(
            "A service manager for running binary files",
        ));
}

#[test]
fn list_table_aligns_unicode_and_caps_long_values_without_changing_storage() {
    use hyperV::{Task, TaskStatus};
    use unicode_width::UnicodeWidthStr;

    let temp = TempDir::new().unwrap();
    let long_path = format!(
        "/Users/{}/run-hourly-monitor.zsh",
        "very-long-directory/".repeat(8)
    );
    let cases = [
        (
            "logger".to_owned(),
            "/bin/bash".to_owned(),
            TaskStatus::Stopped,
        ),
        (
            "desktop-screenshot-watcher".to_owned(),
            long_path.clone(),
            TaskStatus::Running,
        ),
        (
            "selling-spider-hourly".to_owned(),
            long_path.clone(),
            TaskStatus::Failed,
        ),
        (
            "監視🟢e\u{301}".repeat(20),
            format!("/資料/{}/終端.sh", "目錄/".repeat(40)),
            TaskStatus::Stopped,
        ),
        (
            "line\nbreak".to_owned(),
            "/bin/tab\tpath".to_owned(),
            TaskStatus::Stopped,
        ),
        (
            "❤️".repeat(80),
            format!("/{}", "❤️".repeat(80)),
            TaskStatus::Stopped,
        ),
    ];
    let tasks: Vec<_> = cases
        .into_iter()
        .enumerate()
        .map(|(i, (name, binary, status))| {
            let mut task = Task::new(
                format!("{i:08}-0000-0000-0000-000000000000"),
                name,
                binary,
                Vec::new(),
                Default::default(),
                None,
                false,
                None,
                None,
            );
            task.status = status;
            task.last_started = Some("2025-08-07T19:43:06.045078+00:00".to_owned());
            task
        })
        .collect();
    let tasks_path = temp.path().join("tasks.json");
    let stored = serde_json::to_string_pretty(&tasks).unwrap();
    std::fs::write(&tasks_path, &stored).unwrap();

    let output = hyperv_cmd(&temp)
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), tasks.len() + 2);
    assert!(lines.iter().all(|line| line.width() <= 120), "{output}");
    let headers: Vec<_> = ["ID", "NAME", "STATUS", "MEM(MB)", "STARTED", "BINARY"]
        .map(|label| lines[0].find(label).unwrap())
        .into();
    assert_eq!(headers[1], 10, "ID should use only eight columns");
    for (line, task) in lines[2..].iter().zip(&tasks) {
        let status = line.find(task.status.display_with_icon()).unwrap();
        assert_eq!(line[..status].width(), headers[2], "{line}");
        let memory = status + task.status.display_with_icon().len();
        let memory = memory + line[memory..].find('0').unwrap();
        assert_eq!(line[..memory].width(), headers[3], "{line}");
        let started = line.find("2025-08-07").unwrap();
        assert_eq!(line[..started].width(), headers[4], "{line}");
        let binary = line.find('/').unwrap();
        assert_eq!(line[..binary].width(), headers[5], "{line}");
    }
    assert!(lines[3].contains("desktop-screenshot-watcher"));
    assert!(lines[3].ends_with("run-hourly-monitor.zsh"));
    assert!(lines[3].contains("..."));
    assert!(lines[5].ends_with("終端.sh"));
    assert_eq!(std::fs::read_to_string(tasks_path).unwrap(), stored);
}

#[test]
fn test_lifecycle() {
    let temp = TempDir::new().unwrap();
    let logger = abs_repo_path("tests/logger.sh");
    // Stops the spawned logger even if an assertion panics mid-test.
    let _guard = StopGuard {
        config_dir: temp.path().to_path_buf(),
        task: "test-task",
    };

    // 1. Create a task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "test-task", "--binary", &logger])
        .assert()
        .success()
        .stdout(predicate::str::contains("Task created successfully"));

    // 2. List tasks
    hyperv_cmd(&temp)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("test-task"))
        .stdout(predicate::str::contains("Stopped"));

    // 3. Start task
    hyperv_cmd(&temp)
        .args(&["start", "test-task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("started successfully"));

    // 4. Check status
    hyperv_cmd(&temp)
        .args(&["status", "test-task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Task: test-task"));

    // 5. Stop task
    hyperv_cmd(&temp)
        .args(&["stop", "test-task"])
        .assert()
        .success();

    // 6. Remove task
    hyperv_cmd(&temp)
        .args(&["remove", "test-task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    // 7. Verify removed
    hyperv_cmd(&temp)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No tasks configured"));
}

#[test]
fn test_persistence() {
    let temp = TempDir::new().unwrap();
    let ls_bin = bin_path("/bin/ls", "/usr/bin/ls");

    // Create task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "persist-task", "--binary", ls_bin])
        .assert()
        .success();

    // Verify it exists
    hyperv_cmd(&temp)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("persist-task"));

    // "Restart" app - reusing the same temp dir simulates this
    hyperv_cmd(&temp)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("persist-task"));
}

#[test]
fn test_not_found() {
    let temp = TempDir::new().unwrap();
    // `status` on a missing task exits non-zero like every other
    // identifier-based command (start/stop/restart/remove/logs/diagnose),
    // which all return `HyperVError::TaskNotFound`. The runtime prints the
    // `Err` via `Debug`, so assert on the task name in stderr rather than
    // the `Display` wording ("not found").
    hyperv_cmd(&temp)
        .args(&["status", "fake-task"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fake-task"));
}

#[test]
fn test_duplicate_task() {
    let temp = TempDir::new().unwrap();
    let ls_bin = bin_path("/bin/ls", "/usr/bin/ls");
    // 1. Create task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "dup-task", "--binary", ls_bin])
        .assert()
        .success();

    // 2. Create duplicate
    hyperv_cmd(&temp)
        .args(&["new", "--name", "dup-task", "--binary", ls_bin])
        .assert()
        .failure(); // Should exit non-zero
}

#[test]
fn test_long_running() {
    let temp = TempDir::new().unwrap();
    let logger = abs_repo_path("tests/logger.sh");
    // Stops the spawned logger even if an assertion panics mid-test.
    let _guard = StopGuard {
        config_dir: temp.path().to_path_buf(),
        task: "sleeper",
    };

    // Create a long running sleep task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "sleeper", "--binary", &logger])
        .assert()
        .success();

    hyperv_cmd(&temp)
        .args(&["start", "sleeper"])
        .assert()
        .success();

    // Verify it's running
    hyperv_cmd(&temp)
        .args(&["status", "sleeper"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Running"))
        .stdout(predicate::str::contains("PID:"));

    // Stop it
    hyperv_cmd(&temp)
        .args(&["stop", "sleeper"])
        .assert()
        .success();

    // Verify stopped
    hyperv_cmd(&temp)
        .args(&["status", "sleeper"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stopped"));
}

#[test]
fn test_status_refreshes_finished_process() {
    let temp = TempDir::new().unwrap();
    let true_bin = bin_path("/bin/true", "/usr/bin/true");

    hyperv_cmd(&temp)
        .args(&["new", "--name", "oneshot", "--binary", true_bin])
        .assert()
        .success();

    hyperv_cmd(&temp)
        .args(&["start", "oneshot"])
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_millis(200));

    hyperv_cmd(&temp)
        .args(&["status", "oneshot"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Failed"))
        .stdout(predicate::str::contains("Running").not());
}

#[test]
fn test_restart_command() {
    let temp = TempDir::new().unwrap();
    let logger = abs_repo_path("tests/logger.sh");
    // Stops the spawned logger even if an assertion panics mid-test.
    let _guard = StopGuard {
        config_dir: temp.path().to_path_buf(),
        task: "restart-me",
    };

    hyperv_cmd(&temp)
        .args(&["new", "--name", "restart-me", "--binary", &logger])
        .assert()
        .success();

    hyperv_cmd(&temp)
        .args(&["start", "restart-me"])
        .assert()
        .success();

    hyperv_cmd(&temp)
        .args(&["restart", "restart-me"])
        .assert()
        .success();

    // Still running after restart.
    hyperv_cmd(&temp)
        .args(&["status", "restart-me"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Running"))
        .stdout(predicate::str::contains("PID:"));

    // Cleanup so tests don't leak processes.
    let _ = hyperv_cmd(&temp).args(&["stop", "restart-me"]).assert();
}

#[test]
fn test_daemon_locking() {
    let temp = TempDir::new().unwrap();
    let bin_path = assert_cmd::cargo::cargo_bin("hyperV");

    // Kills the daemon on every exit path, including an assertion panic.
    struct Reaper(std::process::Child);
    impl Drop for Reaper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // Start daemon in background using std::process::Command
    let _daemon = Reaper(
        std::process::Command::new(&bin_path)
            .arg("daemon")
            .env("HYPERV_CONFIG_DIR", temp.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );

    // Wait for the daemon to actually hold the PID lock. It writes its pid only after
    // locking, so a non-empty file means the lock is held. A fixed sleep loses this race
    // under load, and losing it means the second daemon starts for real and runs forever.
    let pid_path = temp.path().join("daemon.pid");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !std::fs::read(&pid_path).is_ok_and(|pid| !pid.is_empty()) {
        assert!(
            Instant::now() < deadline,
            "daemon never acquired the PID lock"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    // Try to start another daemon using assert_cmd. The timeout is a backstop: if this one
    // ever does acquire the lock it daemonizes, and without it the test would hang forever
    // instead of failing.
    hyperv_cmd(&temp)
        .arg("daemon")
        .timeout(Duration::from_secs(30))
        .assert()
        .failure() // Should fail
        .stderr(predicate::str::contains("Daemon is already running"));
}
