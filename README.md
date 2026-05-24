# hyperV - Service Manager

A command-line service manager for running and managing binary files on Linux and macOS.

## Features

- ✅ Create and manage tasks/services
- ✅ Start/stop services with graceful shutdown
- ✅ Environment variable support
- ✅ **Automatic .env file loading**
- ✅ Custom working directories
- ✅ Auto-restart configuration
- ✅ Task status monitoring with detailed information
- ✅ Restart command for running services
- ✅ Cross-platform support (Linux & macOS)
- ✅ Persistent task configuration
- ✅ Process monitoring with PID tracking
- ✅ Memory usage in list view (per-process MB)
- ✅ Log management with rotation (>10MB)
- ✅ Real-time log following (--follow flag)
- ✅ Separate stdout/stderr log viewing
- ✅ Enhanced process management with signal handling
- ✅ Process group and child-process cleanup on stop
- ✅ Diagnostic tools for troubleshooting
- ✅ Exit code tracking
- ✅ Restart count monitoring
- ✅ Persistent running-task state across hyperV restarts
- ✅ Compose-style YAML workflow with `up` and `down`

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
hyperV new --name "web-server" --binary "/usr/bin/python3" --args "server.py" "--port=8080"

# Task with environment variables
hyperV new --name "api-service" --binary "/path/to/api" --env "PORT=3000" --env "NODE_ENV=production"

# Task with working directory and auto-restart
hyperV new --name "worker" --binary "/path/to/worker" --workdir "/opt/app" --auto-restart
```

### Using .env files

If a `.env` file exists in the specified working directory (`--workdir`), it will be automatically loaded. Environment variables passed via the `--env` flag will take precedence over the variables in the `.env` file.

**Example `.env` file:**

```
DB_HOST=localhost
DB_USER=myuser
DB_PASS=secret
```

**Creating a task with a `.env` file:**

```bash
hyperV new --name "my-app" \
  --binary "/path/to/app" \
  --workdir "/path/to/my-app-folder" \
  --auto-restart
```

If `/path/to/my-app-folder` contains a `.env` file, the variables within it will be loaded when the `my-app` service is started.

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

`stop` sends SIGTERM first, then SIGKILL if the process does not exit in time. On Unix, hyperV also targets the task process group and known child process groups so helper scripts do not leave the real app running in the background.

### Restart a task

```bash
hyperV restart my-service
```

`restart` stops the task if it is running, then starts it again.

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

# Show a compact diagnostic summary instead of raw lines
hyperV logs my-service --log-type both --summary

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

### Compose-style workflow

Define services in a YAML file, then create or update tasks from it:

```bash
# Uses hyperv.yaml by default
hyperV up --start

# Use a custom file
hyperV up --file ./startup.yaml --start

# Remove services defined in the YAML file
hyperV down --file ./startup.yaml
```

YAML files use this shape:

```yaml
services:
  worker:
    binary: "/bin/bash"
    args: ["./tests/worker.sh"]
    workdir: "/path/to/app"
    env:
      NODE_ENV: "production"
    auto_restart: true
```

## Advanced Features

### Auto-restart
Tasks with `--auto-restart` flag will automatically restart if they fail (up to 5 attempts):

```bash
hyperV new --name "critical-service" --binary "/path/to/service" --auto-restart
```

### Telegram failure alerts
The daemon can send Telegram messages when a service is in real trouble:
- A task crashes 2 times within 10 minutes
- A task exhausts all 5 auto-restart attempts

Alerts are opt-in. Set both environment variables before starting tasks with auto-restart:

```bash
export HYPERV_TELEGRAM_BOT_TOKEN="123456:bot-token"
export HYPERV_TELEGRAM_CHAT_ID="123456789"
hyperV start critical-service
```

Alert messages include the task name, reason, restart count, last exit code, and detection time. They do not include environment variables, raw logs, or command-line arguments.

### Log Management
- Logs are automatically rotated when they exceed 10MB
- Separate stdout and stderr log files
- Real-time log following capability
- Fast last-N reading for tail-like views (reads from end of file efficiently)
- Compact summaries with counts, top repeated messages, recent warnings/errors, and redaction of obvious secret-like values
- Historical log preservation as bounded gzip archives (`stdout.log.1.gz` through `stdout.log.5.gz`, and the same for stderr)

### Process Management
- Graceful shutdown with SIGTERM before SIGKILL
- Process group handling for shell scripts and child processes
- Proper cleanup of zombie processes
- Exit code tracking

### Enhanced Status Information
The status command now shows:
- Last start time
- Restart count
- Last exit code
- Detailed process information

### Runtime State Persistence
hyperV persists the set of running tasks to a small JSON file (running_tasks.json) under the configuration directory. On startup:
- It reads this file and checks each recorded PID to see if the process is still alive.
- Tasks whose PIDs are still running are marked as Running and their PID is restored.
- Stale entries (where the PID is no longer alive) are ignored.

This file is automatically updated when tasks start/stop and when the manager detects that a process has exited. You generally do not need to edit it manually.

## Configuration

Tasks are stored in JSON format at:
- macOS: `~/Library/Application Support/hyperV/tasks.json`
- Linux: `~/.config/hyperV/tasks.json`

Logs are stored in:
- macOS: `~/Library/Application Support/hyperV/logs/<task-id>/`
- Linux: `~/.config/hyperV/logs/<task-id>/`

Runtime state (persisted running tasks) is stored in:
- macOS: `~/Library/Application Support/hyperV/running_tasks.json`
- Linux: `~/.config/hyperV/running_tasks.json`

Daemon coordination state is stored in:
- macOS: `~/Library/Application Support/hyperV/daemon.pid`
- Linux: `~/.config/hyperV/daemon.pid`

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
- `pid_start_time`: Process identity timestamp used to reduce PID-reuse mistakes
- `created_at`: Creation timestamp
- `last_started`: Last start timestamp
- `restart_count`: Number of automatic restarts
- `last_exit_code`: Exit code from last run
- `suppress_restart`: Internal flag that prevents an explicitly stopped task from being auto-restarted
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

### Running SurrealDB

A helper script is provided to launch SurrealDB using environment variables: `tests/surreal.sh`.

Create a task that runs the script via bash:

```bash
hyperV new --name "surrealdb" \
  --binary "/bin/bash" \
  --args "./tests/surreal.sh" \
  --workdir "/path/to/this/repo" \
  --env "SURREAL_HOST=0.0.0.0" \
  --env "SURREAL_PORT=8000" \
  --env "SURREAL_STORAGE_PATH=/tmp/surreal_data" \
  --env "SURREAL_LOG_LEVEL=info" \
  --env "SURREAL_USER=root" \
  --env "SURREAL_PASSWORD=secret" \
  --auto-restart

# Start the database
hyperV start surrealdb

# View logs
hyperV logs surrealdb --follow
```

The script builds and executes the command:

```bash
SURREAL_CMD="surreal start --bind $SURREAL_HOST:$SURREAL_PORT rocksdb:$SURREAL_STORAGE_PATH --log $SURREAL_LOG_LEVEL --user $SURREAL_USER --password $SURREAL_PASSWORD"
```

Ensure the `surreal` binary is available in your PATH. Adjust environment variables as needed.

### Running SurrealDB from hyperv.yaml (compose)

You can also define and run SurrealDB purely from hyperv.yaml without a helper script. Example hyperv.yaml fragment:

```yaml
services:
  surrealdb:
    binary: "/bin/bash"
    args:
      - "-lc"
      - "surreal start --bind $SURREAL_HOST:$SURREAL_PORT rocksdb:$SURREAL_STORAGE_PATH --log $SURREAL_LOG_LEVEL --user $SURREAL_USER --password $SURREAL_PASSWORD"
    env:
      SURREAL_HOST: "0.0.0.0"
      SURREAL_PORT: "8000"
      SURREAL_STORAGE_PATH: "/tmp/surreal_data"
      SURREAL_LOG_LEVEL: "info"
      SURREAL_USER: "root"
      SURREAL_PASSWORD: "secret"
    auto_restart: true
```

Then apply and start it with:

```bash
# create/update tasks from hyperv.yaml and start them
hyperV up -f hyperv.yaml --start

# inspect
hyperV list
hyperV logs surrealdb --follow

# remove the services defined in hyperv.yaml
hyperV down -f hyperv.yaml
```

Notes:
- We use /bin/bash -lc so that environment variables in the command are expanded by the shell.
- The env block in YAML sets those variables for the process; adjust as needed.
- Ensure the surreal binary is available in PATH for the user running hyperV.
- Alternatively, you can keep using the provided tests/surreal.sh wrapper if you prefer.

## Potential Future Enhancements

- [ ] Systemd/launchd integration
- [ ] Web UI for management
- [ ] Task dependencies
- [ ] Scheduling support
- [ ] Richer CPU and resource usage reporting
- [ ] More structured error reporting for automation

## License

MIT License
