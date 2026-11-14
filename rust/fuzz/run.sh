#!/usr/bin/env bash
# Builds and runs the fuzzers with libFuzzer on the STABLE toolchain (coverage
# instrumentation through SanitizerCoverage LLVM flags; no sanitizer, but
# Rust's own bounds and overflow checks are on).
#   bash rust/fuzz/run.sh [seconds per target, default 60] [target ...]
#
# Note: -Cpasses=sancov-module was removed from Rust stable (≥ 1.78); coverage
# instrumentation is passed directly via -Cllvm-args instead.  libfuzzer-sys 0.4
# links the LLVM libFuzzer runtime which picks up these sancov flags automatically.
set -euo pipefail
cd "$(dirname "$0")"
SECS=${1:-60}; shift || true
TARGETS=("$@")
[ ${#TARGETS[@]} -eq 0 ] && TARGETS=(server_text clipboard_text certificate desktop_size files form_fields)
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
export RUSTFLAGS="-Cllvm-args=-sanitizer-coverage-level=4 \
-Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
-Cllvm-args=-sanitizer-coverage-trace-compares -Cdebug-assertions -Coverflow-checks"
cargo build --release --locked --target "$TRIPLE" --bins
for t in "${TARGETS[@]}"; do
  mkdir -p "corpus/$t" "artifacts/$t"
  # Seeds (checked in) plus whatever earlier runs found (not checked in).
  "target/$TRIPLE/release/$t" "corpus/$t" "seeds/$t" \
    -artifact_prefix="artifacts/$t/" -max_total_time="$SECS" -max_len=65536 \
    -print_final_stats=1 2>&1 | grep -E "^(stat::number_of_executed_units|stat::new_units_added|#[0-9]+ +DONE)|panicked|ERROR|SUMMARY" || true
  if compgen -G "artifacts/$t/crash-*" > /dev/null; then
    echo "fuzz: $t CRASHED; inputs in rust/fuzz/artifacts/$t/" >&2
    exit 1
  fi
  echo "fuzz: $t ok (${SECS}s)"
done
