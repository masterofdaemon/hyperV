use assert_cmd::Command;
use predicates::prelude::*;

use tempfile::TempDir;

fn hyperv_cmd(temp_dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("hyperV").unwrap();
    cmd.env("HYPERV_CONFIG_DIR", temp_dir.path());
    cmd
}

#[test]
fn test_help() {
    let temp = TempDir::new().unwrap();
    hyperv_cmd(&temp)
        .arg("--help")
        .assert()
        .success()
        // Check for the "about" text configured in Clap
        .stdout(predicate::str::contains("A service manager for running binary files"));
}

#[test]
fn test_lifecycle() {
    let temp = TempDir::new().unwrap();
    let bin_path = "/bin/ls"; // Use absolute path to ls

    // 1. Create a task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "test-task", "--binary", bin_path])
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
    
    // Create task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "persist-task", "--binary", "/bin/ls"])
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
    // 1. Create task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "dup-task", "--binary", "/bin/ls"])
        .assert()
        .success();

    // 2. Create duplicate
    hyperv_cmd(&temp)
        .args(&["new", "--name", "dup-task", "--binary", "/bin/ls"])
        .assert()
        .failure(); // Should exit non-zero
}

#[test]
fn test_long_running() {
    let temp = TempDir::new().unwrap();

    // Create a long running sleep task
    hyperv_cmd(&temp)
        .args(&["new", "--name", "sleeper", "--binary", "/bin/sleep", "--args", "5"])
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
fn test_daemon_locking() {
    let temp = TempDir::new().unwrap();
    let bin_path = assert_cmd::cargo::cargo_bin("hyperV");
    
    // Start daemon in background using std::process::Command
    let mut child = std::process::Command::new(&bin_path)
        .arg("daemon")
        .env("HYPERV_CONFIG_DIR", temp.path())
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
    let _ = child.kill();
}
