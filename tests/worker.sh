#!/usr/bin/env bash
set -euo pipefail
echo "Worker start $(date)"; for i in $(seq 1 5); do echo "tick $i"; sleep 1; done; echo "Worker done"; sleep 2; echo "Worker exit"; exit 0
