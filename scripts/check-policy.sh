#!/bin/sh
# Security policy checks (SA-6 / SA-7). Fails if forbidden patterns appear in our code.
set -eu
fail=0
# SA-6: never accept arbitrary server certificates.
if grep -rn --include='*.rs' --include='*.dart' --include='*.kt' \
     -e 'DangerouslyAcceptInvalidCertificate' -e 'danger_accept_invalid' \
     -e 'NoCertificateVerification' rust app 2>/dev/null; then
  echo "policy: forbidden 'accept any certificate' code found (SA-6)." >&2
  fail=1
fi
# SA-7: never enable sspi logging (it records credential buffers at debug level).
if grep -rn --include='*.rs' --include='*.dart' --include='*.yml' --include='*.yaml' \
     -E 'sspi=(trace|debug|info|warn)' rust app .github 2>/dev/null; then
  echo "policy: sspi logging must stay off (SA-7)." >&2
  fail=1
fi
[ "$fail" -eq 0 ] && echo "policy: OK"
exit "$fail"
