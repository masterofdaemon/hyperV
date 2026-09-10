# Issues Found in hyperV

## Critical (3)
| # | File | Issue |
|---|------|-------|
| 1 | `src/logs.rs:145` | **`buffer.pop()` corrupts log output** — removes last char before newline instead of the newline itself |
| 2 | `src/config.rs:80` | **`Config::default()` panics** — `expect()` on fallible I/O in `Default` impl |
| 3 | `src/manager.rs:808` | **`TaskManager::default()` panics** — same pattern, multiple I/O ops wrapped in `expect()` |

## High (4)
| # | File | Issue |
|---|------|-------|
| 4 | `src/logs.rs:675-687` | **Sensitive key ordering not longest-first** — `password` (8 chars) matches before `secret_token` (12 chars) |
| 5 | `src/manager.rs:309-320` | **Partial ID matching is ambiguous** — `starts_with("a")` matches every task whose UUID starts with `a` |
| 6 | `src/main.rs:83` | **`Up` command silently swallows start errors** — `let _ = task_manager.start_task(name)` discards failures |
| 7 | `hyperv.yaml:27-28` | **Hardcoded secrets** — `SURREAL_PASSWORD: "secret"` in plaintext |

## Medium (8)
| # | File | Issue |
|---|------|-------|
| 8 | `src/manager.rs:161` | Backup creation failure silently ignored |
| 9 | `src/manager.rs:633-637` | **Race condition** — `check_and_restart_tasks` reads tasks file without locking |
| 10 | `src/manager.rs:751-755` | **Race condition** — `cleanup_with_events` reads without locking |
| 11 | `src/process.rs:39,60,77` | `pid as i32` truncates large PIDs silently |
| 12 | `src/logs.rs:134` | `splice(0..0, ...)` for prepending is O(n²) on large files |
| 13 | `src/logs.rs:182` | `show_logs` with `Both` loses 1 line for odd counts |
| 14 | `src/main.rs:239` | `is_daemon_running` returns `false` when PID file can't be opened (opposite of comment) |
| 15 | `src/main.rs:201-204` | `write_daemon_pid` leaks lock on write failure |

## Low / Code Quality (6)
| # | File | Issue |
|---|------|-------|
| 16 | `src/error.rs` | Duplicate error variants (`Config`/`ConfigError`, `TaskAlreadyExists`/`TaskExists`) |
| 17 | `src/lib.rs:6` | `#![allow(non_snake_case)]` suppresses warnings crate-wide |
| 18 | `src/manager.rs:205` | `env` round-trips through `Vec<String>` → `HashMap` unnecessarily |
| 19 | `examples.sh`, `test_enhanced.sh` | Insecure `/tmp/` file creation (symlink attack vector) |
| 20 | `test_enhanced.sh:48` | Uses `timeout` which doesn't exist on macOS |
| 21 | All shell scripts | Missing `set -euo pipefail` |

## Test Issues (5 notable)
| # | File | Issue |
|---|------|-------|
| 22 | `tests/cli_tests.rs:134` | `test_not_found` asserts exit code 0 for non-existent task (questionable behavior) |
| 23 | `tests/manager_tests.rs:102` | Hardcoded `restart_count = 5` instead of using `MAX_RESTART_ATTEMPTS` constant |
| 24 | `tests/cli_tests.rs` | No panic guard — orphaned processes on test failure |
| 25 | `tests/surreal.sh:17` | Password visible in process listing via CLI arg |
| 26 | `tests/crasher.sh`, `tests/worker.sh` | Unused test scripts (dead code) |

## Priority Fixes

1. **#1** — log corruption (`buffer.pop()` bug)
2. **#2-3** — panics in `Default` impls
3. **#9-10** — race conditions (unlocked reads)
4. **#5** — wrong task selection via partial ID matching
