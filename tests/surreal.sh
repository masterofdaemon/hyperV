#!/usr/bin/env bash
# SurrealDB task launcher for hyperV
# This script composes the SurrealDB start command from environment variables
# and execs it so that hyperV tracks the actual database process PID.

set -euo pipefail

# Defaults (can be overridden by hyperV task env or a .env file in workdir)
: "${SURREAL_HOST:=127.0.0.1}"
: "${SURREAL_PORT:=8000}"
: "${SURREAL_STORAGE_PATH:=/tmp/surrealdb}"
: "${SURREAL_LOG_LEVEL:=info}"
: "${SURREAL_USER:=root}"
: "${SURREAL_PASSWORD:=root}"

# Pass the password via the environment instead of a `--password` CLI arg:
# CLI args are visible in `ps` output, while env vars are not. SurrealDB reads
# the initial root password from SURREAL_PASS.
export SURREAL_PASS="${SURREAL_PASSWORD}"

# Log startup without the secret (never echo the password).
echo "Starting SurrealDB on ${SURREAL_HOST}:${SURREAL_PORT} (user: ${SURREAL_USER}, storage: ${SURREAL_STORAGE_PATH})"

# Ensure storage directory exists
mkdir -p "${SURREAL_STORAGE_PATH}"

# Exec directly (no intermediate `bash -lc "<cmd string>"`) so the secret
# never appears in another process's command line either.
exec surreal start \
  --bind "${SURREAL_HOST}:${SURREAL_PORT}" \
  "rocksdb:${SURREAL_STORAGE_PATH}" \
  --log "${SURREAL_LOG_LEVEL}" \
  --user "${SURREAL_USER}"
