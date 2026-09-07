#!/usr/bin/env bash
# Dogfood seed: Atlas tracks its own remaining build (L3-L9) in its own DAG.
# Usage: ./seed-dogfood.sh [--db ./atlas-dogfood.db]
# Requires: atlas binary built (cargo build).
set -euo pipefail
DB="${1:-./atlas-dogfood.db}"
A="cargo run -q -- --db $DB"
newtask() { # title [dep...] -> prints task id
  local title="$1"; shift
  local args=()
  for d in "$@"; do args+=(--depends-on "$d"); done
  $A task create --title "$title" "${args[@]}" | grep -o "t_[0-9a-f]*" | head -1
}
$A start --goal "Atlas builds itself: L3-L9 modules to verified done" >/dev/null
T2=$(newtask "L3 dispatcher + delegation")
T3=$(newtask "L4 verifier gate" "$T2")
T4=$(newtask "L5 estimator" "$T3")
T5=$(newtask "L6 serve dashboard" "$T3")
T6=$(newtask "L7 forge loop + deploy" "$T3")
T7=$(newtask "L8 xavier adapter" "$T2")
T8=$(newtask "L9 wasm-node parity" "$T2")
echo "seeded into $DB"
$A tree
