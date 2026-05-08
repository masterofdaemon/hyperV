#[cfg(unix)]
mod unix {
    use hyperV::Task;
    use hyperV::process::ProcessManager;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    struct KillGroupOnDrop {
        pgid: u32,
    }

    impl Drop for KillGroupOnDrop {
        fn drop(&mut self) {
            // Best-effort cleanup for test flakiness/panics.
            unsafe {
                libc::kill(-(self.pgid as i32), libc::SIGKILL);
                libc::kill(self.pgid as i32, libc::SIGKILL);
            }
        }
    }

    fn write_executable_script(
        dir: &std::path::Path,
        name: &str,
        body: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write script");

        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");

        path
    }

    fn wait_until<F: Fn() -> bool>(timeout: Duration, f: F) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        f()
    }

    #[test]
    fn stop_task_actually_terminates_process() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");

        let mut pm = ProcessManager::new();

        // Use an absolute path because hyperV validates binaries by path existence.
        let sleep_bin = if std::path::Path::new("/bin/sleep").exists() {
            "/bin/sleep"
        } else {
            "/usr/bin/sleep"
        };

        // Use a long duration and then stop it.
        let task = Task::new(
            "t1".to_string(),
            "sleepy".to_string(),
            sleep_bin.to_string(),
            vec!["60".to_string()],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _guard = KillGroupOnDrop { pgid: pid };

        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_running(pid)),
            "process should be running"
        );

        pm.stop_task(&task.id, pid).expect("stop_task");
        assert!(!pm.is_process_running(pid), "process should be stopped");

        // Stopping again should be a no-op success.
        pm.stop_task(&task.id, pid).expect("stop_task again");
    }

    #[test]
    fn stop_task_kills_process_group_when_leader_exits_early() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");

        let spawner = write_executable_script(
            dir.path(),
            "spawner.sh",
            r#"#!/usr/bin/env bash
set -euo pipefail
sleep 60 &
exit 0
"#,
        );

        let mut pm = ProcessManager::new();
        let task = Task::new(
            "t2".to_string(),
            "spawner".to_string(),
            spawner.to_string_lossy().to_string(),
            vec![],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _guard = KillGroupOnDrop { pgid: pid };

        // The leader may exit quickly; but the group should stay alive because of the background sleep.
        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_group_running(pid)),
            "process group should be running"
        );

        pm.stop_task(&task.id, pid).expect("stop_task");

        assert!(
            !pm.is_process_group_running(pid),
            "process group should be stopped"
        );
    }

    #[test]
    fn stop_task_escalates_to_sigkill_when_process_does_not_exit_after_sigterm() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");

        let mut pm = ProcessManager::new();
        // Use an absolute path because hyperV validates binaries by path existence.
        let bash_bin = if std::path::Path::new("/bin/bash").exists() {
            "/bin/bash"
        } else {
            "/usr/bin/bash"
        };
        let task = Task::new(
            "t3".to_string(),
            "term-ignorer".to_string(),
            bash_bin.to_string(),
            vec![
                "-c".to_string(),
                // Ignore SIGTERM so stop_task has to use SIGKILL.
                "trap '' TERM; while true; do sleep 1; done".to_string(),
            ],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _guard = KillGroupOnDrop { pgid: pid };

        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_running(pid)),
            "process should be running"
        );

        // Prove it ignores SIGTERM (otherwise we'd accidentally only test the SIGTERM path).
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            pm.is_process_running(pid) || pm.is_process_group_running(pid),
            "process/group should still be running after SIGTERM"
        );

        pm.stop_task(&task.id, pid).expect("stop_task");
        assert!(
            !pm.is_process_running(pid) && !pm.is_process_group_running(pid),
            "process/group should be stopped"
        );
    }

    #[test]
    fn stop_task_kills_child_in_separate_process_group() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");
        let child_pid_path = dir.path().join("child.pid");

        let sleep_bin = if std::path::Path::new("/bin/sleep").exists() {
            "/bin/sleep"
        } else {
            "/usr/bin/sleep"
        };
        let python_bin = if std::path::Path::new("/usr/bin/python3").exists() {
            "/usr/bin/python3"
        } else {
            "python3"
        };
        let spawner = dir.path().join("spawn_session_child.py");
        std::fs::write(
            &spawner,
            format!(
                r#"import os
import subprocess

child = subprocess.Popen([{sleep_bin:?}, "60"], preexec_fn=os.setsid)
with open({pid_path:?}, "w") as f:
    f.write(str(child.pid))
    f.flush()
child.wait()
"#,
                sleep_bin = sleep_bin,
                pid_path = child_pid_path.to_string_lossy(),
            ),
        )
        .expect("write python spawner");

        let mut pm = ProcessManager::new();
        let task = Task::new(
            "t6".to_string(),
            "session-child".to_string(),
            python_bin.to_string(),
            vec![spawner.to_string_lossy().to_string()],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _parent_guard = KillGroupOnDrop { pgid: pid };

        assert!(
            wait_until(Duration::from_secs(2), || child_pid_path.exists()),
            "child pid file should be written"
        );
        let child_pid: u32 = std::fs::read_to_string(&child_pid_path)
            .expect("read child pid")
            .parse()
            .expect("parse child pid");
        let _child_guard = KillGroupOnDrop { pgid: child_pid };

        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_running(child_pid)),
            "child should be running"
        );

        pm.stop_task(&task.id, pid).expect("stop_task");

        assert!(
            wait_until(Duration::from_secs(2), || !pm.is_process_running(child_pid)),
            "child in separate process group should be stopped"
        );
    }

    #[test]
    fn pid_matches_identity_fails_for_wrong_binary_without_start_time() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");

        let mut pm = ProcessManager::new();
        let sleep_bin = if std::path::Path::new("/bin/sleep").exists() {
            "/bin/sleep"
        } else {
            "/usr/bin/sleep"
        };

        let task = Task::new(
            "t4".to_string(),
            "sleepy2".to_string(),
            sleep_bin.to_string(),
            vec!["60".to_string()],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _guard = KillGroupOnDrop { pgid: pid };

        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_running(pid)),
            "process should be running"
        );

        let ls_bin = if std::path::Path::new("/bin/ls").exists() {
            "/bin/ls"
        } else {
            "/usr/bin/ls"
        };
        assert!(
            !pm.pid_matches_identity(pid, ls_bin, None),
            "identity check should fail for mismatched binary without start_time"
        );

        pm.stop_task(&task.id, pid).expect("stop_task cleanup");
    }

    #[test]
    fn pid_matches_identity_works_for_scripts_with_start_time() {
        let dir = tempdir().expect("tempdir");
        let stdout = dir.path().join("stdout.log");
        let stderr = dir.path().join("stderr.log");

        // This script runs long enough for sysinfo to observe its cmdline.
        let script = write_executable_script(
            dir.path(),
            "logger.sh",
            r#"#!/usr/bin/env bash
set -euo pipefail
while true; do sleep 1; done
"#,
        );

        let mut pm = ProcessManager::new();
        let task = Task::new(
            "t5".to_string(),
            "script".to_string(),
            script.to_string_lossy().to_string(),
            vec![],
            HashMap::new(),
            Some(dir.path().to_string_lossy().to_string()),
            false,
            Some(stdout.to_string_lossy().to_string()),
            Some(stderr.to_string_lossy().to_string()),
        );

        let pid = pm
            .start_task(&task, &HashMap::new(), &stdout, &stderr)
            .expect("start_task");
        let _guard = KillGroupOnDrop { pgid: pid };

        assert!(
            wait_until(Duration::from_secs(1), || pm.is_process_running(pid)),
            "process should be running"
        );

        let start_time = pm.process_start_time(pid).expect("start_time");
        assert!(
            pm.pid_matches_identity(pid, &task.binary, Some(start_time)),
            "identity check should work with start_time"
        );

        pm.stop_task(&task.id, pid).expect("stop_task cleanup");
    }
}
