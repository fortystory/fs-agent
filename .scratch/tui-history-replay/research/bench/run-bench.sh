#!/usr/bin/env bash
# Throwaway measurement runner for ticket 02.
#
# NOTE: this environment mounts a private /tmp per shell invocation, so the whole
# run (build + measure) has to happen inside ONE process for CARGO_TARGET_DIR in
# /tmp to survive between steps. Run it with a background job and read the job's
# output.
set -euo pipefail
cd "$(dirname "$0")"
export CARGO_TARGET_DIR=/tmp/fs-agent-replay-bench-target
BIN_D="$CARGO_TARGET_DIR/debug/replay-bench"
BIN_R="$CARGO_TARGET_DIR/release/replay-bench"
REAL=/home/forty/.local/share/fs-agent/sessions/-home-forty-code-fortystory-fs-agent-986aee694bc228c4/20260922T134602Z-36f60515/log.jsonl
SYNTH=/tmp/synth-50000.jsonl

echo "== build debug =="
cargo build --offline
echo "== build release =="
cargo build --release --offline

echo "== real session =="
ls -l "$REAL"
wc -l "$REAL"

echo "== synth 50000 =="
"$BIN_R" synth 50000 "$SYNTH"
ls -l "$SYNTH"
wc -l "$SYNTH"

echo "== replay real (debug) =="
"$BIN_D" replay "$REAL" 512
echo "== replay real (release) =="
"$BIN_R" replay "$REAL" 512
echo "== replay synth 50000 (debug) =="
"$BIN_D" replay "$SYNTH" 512
echo "== replay synth 50000 (release) =="
"$BIN_R" replay "$SYNTH" 512

# `evict <pre> <post>`: `evict 0 20000` never crosses the cap (baseline), and
# `evict 20000 20000` evicts one line per push. The difference is the eviction
# cost. (`pre=20000` alone is CAP, so the timed pushes are the ones that evict.)
echo "== evict baseline, below cap (debug) =="
"$BIN_D" evict 0 20000
echo "== evict baseline, below cap (release) =="
"$BIN_R" evict 0 20000
echo "== evict at cap (debug) =="
"$BIN_D" evict 20000 20000
echo "== evict at cap (release) =="
"$BIN_R" evict 20000 20000

echo "== done =="
