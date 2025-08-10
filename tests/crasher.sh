#!/usr/bin/env bash
set -euo pipefail
echo "Crash in 2s"; sleep 2; echo "Boom"; exit 1
