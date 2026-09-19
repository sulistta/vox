#!/usr/bin/env bash
set -euo pipefail

source "${HOME}/.cargo/env" 2>/dev/null || true
pnpm typecheck
pnpm test
python3 -m unittest discover -s tests -p '*_test.py'
bash -n packaging/linux/install.sh packaging/linux/uninstall.sh scripts/validate-bundle.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p vox-desktop
