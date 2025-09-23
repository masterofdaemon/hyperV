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

# Compose command
SURREAL_CMD="surreal start --bind ${SURREAL_HOST}:${SURREAL_PORT} rocksdb:${SURREAL_STORAGE_PATH} --log ${SURREAL_LOG_LEVEL} --user ${SURREAL_USER} --password ${SURREAL_PASSWORD}"

echo "Starting SurrealDB with command:"
echo "  ${SURREAL_CMD}"

# Ensure storage directory exists
mkdir -p "${SURREAL_STORAGE_PATH}"

# Exec the command so this script does not remain as the parent process
exec bash -lc "${SURREAL_CMD}"
