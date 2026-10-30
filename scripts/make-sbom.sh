#!/usr/bin/env bash
# Software bill of materials for the Rust code (S8), CycloneDX 1.5 JSON.
#   bash scripts/make-sbom.sh <output dir>      (needs: cargo install cargo-cyclonedx --version 0.5.7 --locked)
# Package ids contain the build folder; it is replaced by /src so no local path is
# published (SA-9), and the result is checked like the binaries.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
cd "$ROOT"
cargo cyclonedx --format json --spec-version 1.5
# One file per crate, named after the package (rust/core: rdpv-core,
# rust/bridge: rust_lib_cipher_desk).
for pair in core:rdpv-core bridge:rust_lib_cipher_desk; do
  dir=${pair%%:*}; package=${pair#*:}
  src="rust/$dir/$package.cdx.json"
  sed "s#file://$ROOT#file:///src#g" "$src" > "$OUT/sbom-rust-$dir.cdx.json"
  rm "$src"
done
bash "$ROOT/scripts/check-binaries.sh" "$OUT"
