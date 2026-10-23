#!/usr/bin/env bash
# Regenerates ui/src/generated/*.ts from the Rust wire types (yaac-api, feature `ts`).
# Never edit the generated files by hand.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
cargo test -p yaac-api --features ts
