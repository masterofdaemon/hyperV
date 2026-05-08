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
