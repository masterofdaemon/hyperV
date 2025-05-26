# hyperV - Service Manager

A command-line service manager for running and managing binary files on Linux and macOS.

## Features

- ✅ Create and manage tasks/services
- ✅ Start/stop services with graceful shutdown
- ✅ Environment variable support
- ✅ Custom working directories
- ✅ Auto-restart configuration
- ✅ Task status monitoring with detailed information
- ✅ Cross-platform support (Linux & macOS)
- ✅ Persistent task configuration
- ✅ Process monitoring with PID tracking
- ✅ Log management with rotation (>10MB)
- ✅ Real-time log following (--follow flag)
- ✅ Separate stdout/stderr log viewing
- ✅ Enhanced process management with signal handling
- ✅ Diagnostic tools for troubleshooting
- ✅ Exit code tracking
- ✅ Restart count monitoring

## Installation

1. Clone the repository
2. Build the project:
   ```bash
   cargo build --release
   ```
3. The binary will be available at `target/release/hyperV`

## Usage

### Create a new task

```bash
# Basic task
hyperV new --name "my-service" --binary "/path/to/binary"

# Task with arguments
hyperV new --name "web-server" --binary "/usr/bin/python3" --args "server.py" --args "--port=8080"

# Task with environment variables
hyperV new --name "api-service" --binary "/path/to/api" --env "PORT=3000" --env "NODE_ENV=production"

# Task with working directory and auto-restart
hyperV new --name "worker" --binary "/path/to/worker" --workdir "/opt/app" --auto-restart
```

### List all tasks

```bash
hyperV list
```

### Start a task

```bash
hyperV start my-service
# or by partial ID
hyperV start abff48b0
```

### Stop a task

```bash
hyperV stop my-service
```

### Show task status

```bash
# Show specific task
hyperV status my-service

# Show all tasks
hyperV status
```

### Remove a task

```bash
hyperV remove my-service
```

### View logs

```bash
# Show last 50 lines of stdout (default)
hyperV logs my-service

# Show last 100 lines of stdout
hyperV logs my-service --lines 100

# Show stderr logs
hyperV logs my-service --log-type stderr

# Show both stdout and stderr
hyperV logs my-service --log-type both

# Follow logs in real-time (like tail -f)
hyperV logs my-service --follow

# Follow stderr logs in real-time
hyperV logs my-service --log-type stderr --follow
```

### Diagnose task issues

```bash
# Analyze binary file and configuration for issues
hyperV diagnose my-service
```

## Advanced Features

### Auto-restart
Tasks with `--auto-restart` flag will automatically restart if they fail (up to 5 attempts):

```bash
hyperV new --name "critical-service" --binary "/path/to/service" --auto-restart
```

### Log Management
- Logs are automatically rotated when they exceed 10MB
- Separate stdout and stderr log files
- Real-time log following capability
- Historical log preservation (.old files)

### Process Management
- Graceful shutdown with SIGTERM before SIGKILL
- Process group handling for shell scripts
- Proper cleanup of zombie processes
- Exit code tracking

### Enhanced Status Information
The status command now shows:
- Last start time
- Restart count
- Last exit code
- Detailed process information

## Configuration

Tasks are stored in JSON format at:
- macOS: `~/Library/Application Support/hyperV/tasks.json`
- Linux: `~/.config/hyperV/tasks.json`

Logs are stored in:
- macOS: `~/Library/Application Support/hyperV/logs/<task-id>/`
- Linux: `~/.config/hyperV/logs/<task-id>/`

## Task Structure

Each task contains:
- `id`: Unique identifier (UUID)
- `name`: Human-readable name
- `binary`: Path to executable
- `args`: Command-line arguments
- `env`: Environment variables
- `workdir`: Working directory (optional)
- `auto_restart`: Auto-restart on failure
- `status`: Current status (Running/Stopped/Failed)
- `pid`: Process ID when running
- `created_at`: Creation timestamp
- `last_started`: Last start timestamp
- `restart_count`: Number of automatic restarts
- `last_exit_code`: Exit code from last run
- `stdout_log_path`: Path to stdout log file
- `stderr_log_path`: Path to stderr log file

## Examples

### Running a Python web server

```bash
hyperV new --name "flask-app" \
  --binary "/usr/bin/python3" \
  --args "app.py" \
  --env "FLASK_ENV=production" \
  --env "PORT=5000" \
  --workdir "/opt/webapp" \
  --auto-restart

hyperV start flask-app
```

### Running a Node.js service

```bash
hyperV new --name "node-api" \
  --binary "/usr/bin/node" \
  --args "index.js" \
  --env "NODE_ENV=production" \
  --env "PORT=3000" \
  --auto-restart

hyperV start node-api
```

### Running a custom binary

```bash
hyperV new --name "my-daemon" \
  --binary "/usr/local/bin/mydaemon" \
  --args "--config" \
  --args "/etc/mydaemon.conf" \
  --auto-restart

hyperV start my-daemon
```

## Future Enhancements

- [ ] Log file management and viewing
- [ ] Process monitoring with automatic restart
- [ ] Resource usage tracking
- [ ] Systemd/launchd integration
- [ ] Web UI for management
- [ ] Task dependencies
- [ ] Scheduling support
- [ ] Better error handling and recovery

## License

MIT License
