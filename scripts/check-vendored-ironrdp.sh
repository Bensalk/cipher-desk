#!/usr/bin/env bash
# Checks our copy of ironrdp-server (rust/third_party/ironrdp-server) against
# the real crates.io release it was copied from, and fails if anything beyond
# the changes README-CIPHER-DESK.md documents has drifted — a file nobody
# meant to touch, or a file that quietly appeared or disappeared. This is the
# one dependency `cargo deny`/OSV-Scanner cannot see, because it is a local
# path dependency, not something pulled from crates.io at build time.
#
# Usage: scripts/check-vendored-ironrdp.sh
set -euo pipefail
cd "$(dirname "$0")/.."

VENDOR_DIR="rust/third_party/ironrdp-server"
UPSTREAM_VERSION="0.13.0"
# SHA-256 of the .crate file for ironrdp-server 0.13.0, as published on
# crates.io. Pinned so a compromised or re-published mirror can't swap in
# different source for us to "pass" against. Update this (and
# UPSTREAM_VERSION) only as part of deliberately re-basing the copy, and say
# so in README-CIPHER-DESK.md's change list.
EXPECTED_SHA256="1d3922b23ad9f262b932d42fa37df87f7e45c2d12f45433c0cc084f8762fb09a"

# Files README-CIPHER-DESK.md says we changed. If the real diff names any
# other file, that's undocumented drift and this script fails.
EXPECTED_CHANGED=("src/encoder/mod.rs" "src/encoder/rfx.rs" "src/lib.rs" "src/server.rs")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

crate_url="https://crates.io/api/v1/crates/ironrdp-server/${UPSTREAM_VERSION}/download"
echo "check-vendored-ironrdp: downloading ironrdp-server ${UPSTREAM_VERSION} from crates.io..."
if ! curl -sSL -o "$work/ironrdp-server.crate" "$crate_url"; then
  echo "check-vendored-ironrdp: could not download $crate_url (network unavailable?)." >&2
  exit 1
fi

actual_sha256=$(sha256sum "$work/ironrdp-server.crate" | awk '{print $1}')
if [ "$actual_sha256" != "$EXPECTED_SHA256" ]; then
  echo "check-vendored-ironrdp: SHA-256 mismatch for ironrdp-server ${UPSTREAM_VERSION}." >&2
  echo "  expected: $EXPECTED_SHA256" >&2
  echo "  got:      $actual_sha256" >&2
  echo "  Refusing to diff against it. If this is a deliberate version bump," >&2
  echo "  update EXPECTED_SHA256 by hand after checking the new hash yourself" >&2
  echo "  (e.g. against the hash crates.io's own web page shows for that version)." >&2
  exit 1
fi

tar -xzf "$work/ironrdp-server.crate" -C "$work"
upstream_src="$work/ironrdp-server-${UPSTREAM_VERSION}/src"
if [ ! -d "$upstream_src" ]; then
  echo "check-vendored-ironrdp: no src/ in the downloaded crate; layout changed?" >&2
  exit 1
fi

# Same set of .rs files on both sides (none added, none removed, no rename).
upstream_list=$(cd "$upstream_src" && find . -name '*.rs' | sed 's#^\./##' | sort)
ours_list=$(cd "$VENDOR_DIR/src" && find . -name '*.rs' | sed 's#^\./##' | sort)
if [ "$upstream_list" != "$ours_list" ]; then
  echo "check-vendored-ironrdp: the set of source files differs from upstream ${UPSTREAM_VERSION}:" >&2
  diff <(echo "$upstream_list") <(echo "$ours_list") >&2 || true
  echo "check-vendored-ironrdp: update README-CIPHER-DESK.md's change list, or restore the missing file." >&2
  exit 1
fi

# Every file that actually differs must be one of the documented ones.
changed=()
while IFS= read -r f; do
  if ! diff -q "$upstream_src/$f" "$VENDOR_DIR/src/$f" >/dev/null 2>&1; then
    changed+=("src/$f")
  fi
done <<<"$upstream_list"

undocumented=()
for f in "${changed[@]}"; do
  found=0
  for expected in "${EXPECTED_CHANGED[@]}"; do
    [ "$f" = "$expected" ] && found=1 && break
  done
  [ "$found" -eq 0 ] && undocumented+=("$f")
done

if [ "${#undocumented[@]}" -gt 0 ]; then
  echo "check-vendored-ironrdp: file(s) differ from upstream ${UPSTREAM_VERSION} that" >&2
  echo "README-CIPHER-DESK.md does not mention changing:" >&2
  for f in "${undocumented[@]}"; do
    echo "  $f" >&2
    diff -u "$upstream_src/${f#src/}" "$VENDOR_DIR/$f" >&2 || true
  done
  echo "check-vendored-ironrdp: either revert the unexpected change, or document it" >&2
  echo "in README-CIPHER-DESK.md's change list and add it to EXPECTED_CHANGED here." >&2
  exit 1
fi

for f in "${changed[@]}"; do
  echo "check-vendored-ironrdp: $f differs from upstream, as documented."
done
echo "check-vendored-ironrdp: OK (no undocumented drift from ironrdp-server ${UPSTREAM_VERSION})."
