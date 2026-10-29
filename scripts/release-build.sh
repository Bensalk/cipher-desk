#!/bin/sh
# Release build of the Rust core with local folder paths removed from binaries (SA-9).
# Usage: sh scripts/release-build.sh [extra cargo args]
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME_DIR=${RUSTUP_HOME:-$HOME/.rustup}
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$ROOT=/src --remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$RUSTUP_HOME_DIR=/rustup --remap-path-prefix=$HOME=/home"
cd "$ROOT"
cargo build --release --workspace "$@"
