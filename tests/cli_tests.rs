#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::zombie_processes,
    deprecated
)]

use assert_cmd::Command;
use predicates::prelude::*;

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
fn test_lifecycle() {
    let temp = TempDir::new().unwrap();
    let logger = abs_repo_path("tests/logger.sh");

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
    hyperv_cmd(&temp)
        .args(&["status", "fake-task"])
        .assert()
        .success() // CLI returns 0 but prints valid message usually
        .stdout(predicate::str::contains("not found"));
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

    // Start daemon in background using std::process::Command
    let mut child = std::process::Command::new(&bin_path)
        .arg("daemon")
        .env("HYPERV_CONFIG_DIR", temp.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    // Give it a moment to start and lock
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Try to start another daemon using assert_cmd
    hyperv_cmd(&temp)
        .arg("daemon")
        .assert()
        .failure() // Should fail
        .stderr(predicate::str::contains("Daemon is already running"));

    // Clean up
    #[cfg(unix)]
    {
        use std::time::{Duration, Instant};
        let pid = child.id() as i32;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
            libc::kill(pid, libc::SIGKILL);
        }

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("daemon did not terminate after SIGKILL");
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.try_wait();
    }
}
