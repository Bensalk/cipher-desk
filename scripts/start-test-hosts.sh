#!/usr/bin/env bash
# Starts local RDP test hosts for the live integration tests (rust/core/tests/live_host.rs):
#   - FreeRDP shadow server on port 3389 with NLA required (positive tests),
#   - xrdp on port 3390, which does not support NLA (must be refused).
# A random password is generated for every run; it is never written to the repository.
#
# Usage:   source scripts/start-test-hosts.sh   (bash, Linux; needs: Xvfb xterm freerdp3-shadow-x11
#                                           winpr3-utils (winpr-hash3) xrdp xorgxrdp openssl)
# In GitHub Actions the variables are written to $GITHUB_ENV and the password is masked.
set -eu

WORK=$(mktemp -d)
chmod 700 "$WORK"
DISPLAY_NUM=${RDPV_TEST_DISPLAY:-99}
USER_NAME=rdpvtest
PASSWORD=$(openssl rand -base64 24 | tr -d '/+=' | cut -c1-24)

if [ -n "${GITHUB_ACTIONS:-}" ]; then
  echo "::add-mask::$PASSWORD"
fi

# A virtual screen with something on it for the shadow server to share.
Xvfb ":$DISPLAY_NUM" -screen 0 1280x800x24 >"$WORK/xvfb.log" 2>&1 &
sleep 2
DISPLAY=":$DISPLAY_NUM" xterm -geometry 100x30+50+50 -e 'echo RDPV-TEST-HOST; sleep 100000' >/dev/null 2>&1 &

# NLA host: credentials checked against an NTLM SAM file.
umask 077
winpr-hash3 -u "$USER_NAME" -p "$PASSWORD" -f sam >"$WORK/SAM"
(cd "$WORK" && DISPLAY=":$DISPLAY_NUM" setsid freerdp-shadow-cli3 /port:3389 \
  /sam-file:"$WORK/SAM" /sec:nla >"$WORK/shadow.log" 2>&1 &)

# Host without NLA (xrdp), if installed.
NONLA_PORT=""
if command -v xrdp >/dev/null 2>&1; then
  sudo sed -i 's/^port=3389$/port=3390/' /etc/xrdp/xrdp.ini
  sudo mkdir -p /run/xrdp/sockdir && sudo chmod 1777 /run/xrdp/sockdir
  sudo rm -f /var/run/xrdp/xrdp.pid /var/run/xrdp/xrdp-sesman.pid
  (sudo setsid /usr/sbin/xrdp-sesman >/dev/null 2>&1 &)
  sleep 1
  (sudo setsid /usr/sbin/xrdp >/dev/null 2>&1 &)
  NONLA_PORT=3390
fi

# Wait for the ports.
for port in 3389 ${NONLA_PORT}; do
  i=0
  until (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; do
    i=$((i + 1))
    [ "$i" -gt 30 ] && { echo "test host on port $port did not start" >&2; exit 1; }
    sleep 1
  done
done

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "RDPV_TEST_HOST=127.0.0.1"
    echo "RDPV_TEST_PORT=3389"
    echo "RDPV_TEST_USER=$USER_NAME"
    echo "RDPV_TEST_PASSWORD=$PASSWORD"
    if [ -n "$NONLA_PORT" ]; then echo "RDPV_TEST_NONLA_PORT=$NONLA_PORT"; fi
  } >>"$GITHUB_ENV"
else
  export RDPV_TEST_HOST=127.0.0.1 RDPV_TEST_PORT=3389 RDPV_TEST_USER="$USER_NAME"
  export RDPV_TEST_PASSWORD="$PASSWORD"
  if [ -n "$NONLA_PORT" ]; then export RDPV_TEST_NONLA_PORT="$NONLA_PORT"; fi
fi
echo "Test hosts ready (NLA on 3389${NONLA_PORT:+, no-NLA on $NONLA_PORT})."
