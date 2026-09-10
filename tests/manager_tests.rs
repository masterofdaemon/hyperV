use hyperV::{Task, TaskManager, TaskStatus};
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn bin_path(primary: &'static str, fallback: &'static str) -> &'static str {
    if std::path::Path::new(primary).exists() {
        primary
    } else {
        fallback
    }
}

fn with_temp_config<T>(f: impl FnOnce(&TempDir) -> T) -> T {
    let _guard = env_lock().lock().unwrap();
    let temp = TempDir::new().unwrap();
    let previous = std::env::var_os("HYPERV_CONFIG_DIR");

    // Environment mutation is process-global, so this helper serializes all direct
    // TaskManager tests that need to point at a temporary config directory.
    unsafe {
        std::env::set_var("HYPERV_CONFIG_DIR", temp.path());
    }

    let result = f(&temp);

    unsafe {
        if let Some(previous) = previous {
            std::env::set_var("HYPERV_CONFIG_DIR", previous);
        } else {
            std::env::remove_var("HYPERV_CONFIG_DIR");
        }
    }

    result
}

fn read_tasks(temp: &TempDir) -> Vec<Task> {
    let tasks_path = temp.path().join("tasks.json");
    let content = std::fs::read_to_string(tasks_path).unwrap();
    serde_json::from_str(&content).unwrap()
}

#[test]
fn stale_manager_does_not_overwrite_newer_tasks_on_create() {
    with_temp_config(|temp| {
        let true_bin = bin_path("/bin/true", "/usr/bin/true");
        let mut first = TaskManager::new().unwrap();
        let mut stale = TaskManager::new().unwrap();

        first
            .create_task(
                "first".to_string(),
                true_bin.to_string(),
                Vec::new(),
                Vec::new(),
                None,
                false,
            )
            .unwrap();

        stale
            .create_task(
                "second".to_string(),
                true_bin.to_string(),
                Vec::new(),
                Vec::new(),
                None,
                false,
            )
            .unwrap();

        let mut names: Vec<String> = read_tasks(temp).into_iter().map(|task| task.name).collect();
        names.sort();
        assert_eq!(names, vec!["first", "second"]);
    });
}

#[test]
fn exhausted_auto_restart_task_is_not_restarted_again() {
    with_temp_config(|temp| {
        let true_bin = bin_path("/bin/true", "/usr/bin/true");
        let mut manager = TaskManager::new().unwrap();
        manager
            .create_task(
                "exhausted".to_string(),
                true_bin.to_string(),
                Vec::new(),
                Vec::new(),
                None,
                true,
            )
            .unwrap();

        let tasks_path = temp.path().join("tasks.json");
        let mut tasks = read_tasks(temp);
        tasks[0].status = TaskStatus::Failed;
        tasks[0].restart_count = 5;
        std::fs::write(&tasks_path, serde_json::to_string_pretty(&tasks).unwrap()).unwrap();

        manager.check_and_restart_tasks().unwrap();

        let tasks = read_tasks(temp);
        assert_eq!(tasks[0].restart_count, 5);
        assert_eq!(tasks[0].status, TaskStatus::Failed);
        assert!(tasks[0].pid.is_none());
    });
}

#[test]
fn mutations_reload_state_and_reject_ambiguous_prefixes() {
    with_temp_config(|temp| {
        let mut manager = TaskManager::new().unwrap();
        let mut stale = TaskManager::new().unwrap();
        for name in ["first", "second"] {
            manager
                .create_task(
                    name.into(),
                    "sleep".into(),
                    vec!["60".into()],
                    vec![],
                    None,
                    false,
                )
                .unwrap();
        }
        let mut tasks = read_tasks(temp);
        tasks[0].id = "a".into();
        tasks[1].id = "abc".into();
        std::fs::write(
            temp.path().join("tasks.json"),
            serde_json::to_vec(&tasks).unwrap(),
        )
        .unwrap();
        // Exact IDs beat prefixes, including short IDs that list_tasks must safely display.
        stale.list_tasks();
        stale.stop_task("a").unwrap();
        tasks[0].id = "abd".into();
        std::fs::write(
            temp.path().join("tasks.json"),
            serde_json::to_vec(&tasks).unwrap(),
        )
        .unwrap();
        for result in [
            stale.start_task("ab"),
            stale.stop_task("ab"),
            stale.restart_task("ab"),
            stale.remove_task("ab"),
        ] {
            assert!(result.unwrap_err().to_string().contains("Ambiguous"));
        }
        stale.remove_task("abd").unwrap();
        assert_eq!(read_tasks(temp)[0].name, "second");
        manager.cleanup_with_events().unwrap();
        manager.check_and_restart_tasks().unwrap();
        assert_eq!(manager.task_count(), 1);
    });
}

#[test]
fn compose_stops_before_update_and_reports_partial_teardown() {
    use hyperV::compose::{ComposeFile, Service};
    with_temp_config(|temp| {
        let mut manager = TaskManager::new().unwrap();
        let service = Service {
            binary: "sleep".into(),
            args: vec!["60".into()],
            env: Default::default(),
            workdir: None,
            auto_restart: false,
        };
        let mut compose = ComposeFile {
            services: [("worker".into(), service)].into(),
        };
        manager.up_from_compose(&compose).unwrap();
        manager.start_task("worker").unwrap();
        let pid = read_tasks(temp)[0].pid.unwrap();
        compose.services.get_mut("worker").unwrap().binary = "/bin/echo".into();
        manager.up_from_compose(&compose).unwrap();
        assert!(!hyperV::process::ProcessManager::new().is_process_running(pid));
        let mut tasks = read_tasks(temp);
        assert_eq!(tasks[0].status, TaskStatus::Stopped);
        assert_eq!(tasks[0].binary, "/bin/echo");
        // A corrupt Running record must remain present and cause down to fail.
        tasks[0].status = TaskStatus::Running;
        tasks[0].pid = None;
        std::fs::write(
            temp.path().join("tasks.json"),
            serde_json::to_vec(&tasks).unwrap(),
        )
        .unwrap();
        let service = compose.services["worker"].clone();
        compose.services.insert("other".into(), service.clone());
        manager
            .create_task("other".into(), service.binary, vec![], vec![], None, false)
            .unwrap();
        assert!(
            manager
                .down_from_compose(&compose)
                .unwrap_err()
                .to_string()
                .contains("worker")
        );
        assert_eq!(read_tasks(temp).len(), 1);
    });
}

#[test]
fn concurrent_starts_spawn_only_once() {
    with_temp_config(|_| {
        let mut manager = TaskManager::new().unwrap();
        manager
            .create_task(
                "worker".into(),
                "sleep".into(),
                vec!["60".into()],
                vec![],
                None,
                false,
            )
            .unwrap();
        let first = TaskManager::new().unwrap();
        let second = TaskManager::new().unwrap();
        let barrier = std::sync::Barrier::new(2);
        let mut results = std::thread::scope(|scope| {
            let handles: Vec<_> = [first, second]
                .into_iter()
                .map(|mut manager| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        let result = manager.start_task("worker");
                        (manager, result)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for (manager, result) in &mut results {
            if result.is_ok() {
                manager.stop_task("worker").unwrap();
            }
        }
        assert_eq!(
            results.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        assert!(results.iter().any(|(_, result)| matches!(
            result,
            Err(hyperV::error::HyperVError::TaskAlreadyRunning(_))
        )));
    });
}
