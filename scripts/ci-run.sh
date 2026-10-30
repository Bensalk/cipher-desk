#!/usr/bin/env bash
# Runs a CI command and, if it fails, shows its error lines as a GitHub annotation.
# Job logs are not always easy to reach, so the annotation carries the important part.
#
# Usage:  bash scripts/ci-run.sh "<title>" <command> [args...]
# Secrets masked with ::add-mask:: are masked in annotations too.
set -uo pipefail

title=$1
shift
log=$(mktemp)
"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
if [ "$status" -ne 0 ]; then
  {
    grep -n -i -E -B2 -A6 \
      'error[:\[ ]|error:|FAILED|failed|panicked|undefined reference|cannot find|not found|Exception|Expected:|Actual:|Timed out|What went wrong|e: file|\[E\]|assert' \
      "$log" | grep -v -E '^[0-9]+-\s*$' | head -n 120
    echo '---- last lines ----'
    tail -n 30 "$log"
  } | cut -c1-300 | sed -e 's/%/%25/g' -e 's/\r//g' | awk '{printf "%s%%0A", $0}' >"$log.ann"
  echo "::error title=${title}::$(cat "$log.ann")"
fi
rm -f "$log" "$log.ann"
exit "$status"
