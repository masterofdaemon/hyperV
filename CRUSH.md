# CRUSH.md

## Build
`cargo build --release`

## Test
`cargo test`

## Lint
`cargo clippy --all-targets`

## Code Style
- Format with `rustfmt`
- Snake case module names (task.rs)
- Prefer Result/Option for errors
- Follow Rust API guidelines

## Structure
- Tasks: task.rs
- Process mgmt: process.rs
- Config: config.rs
- Logs: logs.rs
- CLI: cli.rs