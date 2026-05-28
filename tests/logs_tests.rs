use hyperV::constants::{MAX_LOG_ARCHIVES, MAX_LOG_SIZE};
use hyperV::logs::{LogManager, LogType};
use std::fs;
use tempfile::TempDir;

fn write_oversized_log(path: &std::path::Path, marker: u8) {
    let mut bytes = vec![marker; (MAX_LOG_SIZE + 1) as usize];
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

#[test]
fn rotates_logs_into_bounded_gzip_archives() {
    let temp = TempDir::new().unwrap();
    let log_path = temp.path().join("stdout.log");

    for index in 0..(MAX_LOG_ARCHIVES + 2) {
        write_oversized_log(&log_path, b'a' + index as u8);

        LogManager::rotate_log_if_needed(&log_path).unwrap();

        assert!(!log_path.exists());
        let newest_archive = temp.path().join("stdout.log.1.gz");
        assert!(newest_archive.exists());

        let archive_bytes = fs::read(newest_archive).unwrap();
        assert_eq!(&archive_bytes[..2], &[0x1f, 0x8b]);
    }

    for archive_index in 1..=MAX_LOG_ARCHIVES {
        assert!(
            temp.path()
                .join(format!("stdout.log.{archive_index}.gz"))
                .exists()
        );
    }
    assert!(
        !temp
            .path()
            .join(format!("stdout.log.{}.gz", MAX_LOG_ARCHIVES + 1))
            .exists()
    );
}

#[test]
fn summarizes_logs_with_counts_repeated_messages_and_redaction() {
    let temp = TempDir::new().unwrap();
    let stdout_path = temp.path().join("stdout.log");
    let stderr_path = temp.path().join("stderr.log");

    fs::write(
        &stdout_path,
        [
            r#"{"time":"2026-05-21T10:00:00+03:00","level":"INFO","msg":"service ready"}"#,
            r#"{"time":"2026-05-21T10:01:00+03:00","level":"WARN","msg":"OpenAI API error api_key=sk-test-secret"}"#,
            r#"{"time":"2026-05-21T10:02:00+03:00","level":"WARN","message":"OpenAI API error password=hunter2"}"#,
            r#"{"time":"2026-05-21T10:03:00+03:00","level":"ERROR","msg":"failed to process request token=abc123"}"#,
        ]
        .join("\n"),
    )
    .unwrap();
    fs::write(
        &stderr_path,
        [
            "WARN plain warning",
            "ERROR plain failure with SECRET_TOKEN=abc123",
        ]
        .join("\n"),
    )
    .unwrap();

    let summary = LogManager::summarize_logs(&stdout_path, &stderr_path, LogType::Both).unwrap();
    let formatted = summary.format();

    assert_eq!(summary.total_lines, 6);
    assert_eq!(summary.warning_count, 3);
    assert_eq!(summary.error_count, 2);
    assert!(formatted.contains("Top repeated messages"));
    assert!(formatted.contains("OpenAI API error api_key=[REDACTED]"));
    assert!(formatted.contains("OpenAI API error password=[REDACTED]"));
    assert!(formatted.contains("Recent warnings/errors"));
    assert!(formatted.contains("failed to process request token=[REDACTED]"));
    assert!(formatted.contains("SECRET_TOKEN=[REDACTED]"));
    assert!(!formatted.contains("sk-test-secret"));
    assert!(!formatted.contains("hunter2"));
    assert!(!formatted.contains("abc123"));
}
