#!/bin/sh
# Release build of the Rust core with local folder paths removed from binaries (SA-9).
# Usage: sh scripts/release-build.sh [extra cargo args]
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME_DIR=${RUSTUP_HOME:-$HOME/.rustup}
# rustc applies the LAST matching rule, so the broadest prefix ($HOME) comes first
# and the most specific ones last (checked by scripts/check-binaries.sh).
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home --remap-path-prefix=$RUSTUP_HOME_DIR=/rustup --remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$ROOT=/src"
cd "$ROOT"
cargo build --release --workspace "$@"
