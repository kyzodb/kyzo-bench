#!/usr/bin/env bash
# The consistency kill shot, pipeline side, end to end:
#   ./run.sh [seed] [reads]
# Brings the four-store pipeline up, runs the seeded workload, prints the
# anomaly count and first witnesses, tears the pipeline down.
set -euo pipefail

SEED="${1:-35001}"
READS="${2:-2000}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

cleanup() { docker compose down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker compose up --wait

if [[ ! -d .venv ]]; then
    python3 -m venv .venv
    ./.venv/bin/python -m pip install --quiet -r requirements.txt
fi

exec_time="$(date -u +%Y%m%dT%H%M%SZ)"
timeout 600 ./.venv/bin/python driver.py \
    --seed "$SEED" --reads "$READS" \
    --out "witness-${exec_time}-seed${SEED}.json"
