#!/usr/bin/env bash
# SA-9: release files must not contain local folder paths (which reveal the
# builder's user name). Scans every file under the given folders/files for home
# directory or Windows profile paths, allowing only the anonymous CI runner's.
#   bash scripts/check-binaries.sh <dir-or-file> ...
set -euo pipefail
found=0
pattern='(/home/[A-Za-z0-9._-]+/|/Users/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[^\\]+\\|/root/)'
# The CI runner's account (runneradmin) also appears in its short 8.3 form RUNNER~1.
allowed='(/home/runner/|\\Users\\runneradmin\\|\\Users\\RUNNER~1\\)'
while IFS= read -r -d '' f; do
  hits=$(grep -a -o -E "$pattern" "$f" 2>/dev/null | grep -v -E "$allowed" | sort -u | head -5 || true)
  if [ -n "$hits" ]; then
    echo "local path(s) in $f:" >&2
    echo "$hits" | sed 's/^/    /' >&2
    found=1
  fi
done < <(find "$@" -type f -print0)
if [ "$found" -ne 0 ]; then
  echo "check-binaries: FAILED — rebuild with path remapping (scripts/release-build.sh / RUSTFLAGS)" >&2
  exit 1
fi
echo "check-binaries: OK (no local paths)"
