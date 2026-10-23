#!/bin/sh
# Runs every engine harness and the harness self-test against the built bundle, then the restart
# persistence check. Exits non-zero on the first failure. Run `scripts/bundle-macos.sh` (macOS) or
# `scripts/bundle-linux.sh` (Linux) first.
#
# Each harness must exit 0 AND print `HARNESS result=pass`. The self-test (a forced failure after the
# window closes) must exit non-zero, which shows the harness verdict reaches the exit code.
#
# Linux: the GUI harnesses run in a NESTED headless Hyprland session, never the owner's session. The
# script refuses to start unless the environment is that session (see `guard_linux`), and it runs
# the app with HOME and the XDG directories under YAAC_TEST_HOME, so no app state lands in the
# owner's home. Start the session with the nested-compositor scripts first.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
os=$(uname -s)
export YAAC_MOCK_KEYCHAIN=1
fail=0
log=$(mktemp -d)

case "$os" in
    Darwin) app=${APP:-$root/target/bundle/yaac-app.app/Contents/MacOS/yaac-app} ;;
    Linux)
        app=${APP:-$root/target/bundle-linux/yaac-app}
        : "${YAAC_TEST_HOME:?Linux: set YAAC_TEST_HOME to a scratch directory outside the real home data}"
        ;;
    *) echo "run-harnesses: unsupported OS $os" >&2; exit 2 ;;
esac

# The owner's session must never receive a window or input from these runs. Refuse unless every
# variable points at the nested headless session (scripts/nested/env.sh sets them).
guard_linux() {
    case "${XDG_RUNTIME_DIR:-}" in
        */yaac-nested) ;;
        *) echo "run-harnesses: XDG_RUNTIME_DIR must be the nested session directory (*/yaac-nested)" >&2; exit 2 ;;
    esac
    [ -n "${WAYLAND_DISPLAY:-}" ] && [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || {
        echo "run-harnesses: WAYLAND_DISPLAY is not the nested socket" >&2; exit 2; }
    [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && [ -d "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE" ] || {
        echo "run-harnesses: HYPRLAND_INSTANCE_SIGNATURE is not the nested instance" >&2; exit 2; }
    case "${DBUS_SESSION_BUS_ADDRESS:-}" in
        *yaac-nested/no-bus) ;;
        *) echo "run-harnesses: DBUS_SESSION_BUS_ADDRESS must be the dead nested bus" >&2; exit 2 ;;
    esac
    # Isolation: the app's HOME, XDG and temp directories all live under YAAC_TEST_HOME.
    export HOME=$YAAC_TEST_HOME/home
    export XDG_CONFIG_HOME=$YAAC_TEST_HOME/config
    export XDG_DATA_HOME=$YAAC_TEST_HOME/data
    export XDG_CACHE_HOME=$YAAC_TEST_HOME/cache
    export XDG_STATE_HOME=$YAAC_TEST_HOME/state
    export TMPDIR=$YAAC_TEST_HOME/tmp
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMPDIR"
    log=$(mktemp -d "$TMPDIR/harness-logs.XXXXXX")
}

[ "$os" = Linux ] && guard_linux

# No process of this bundle may outlive a stage: zygotes and renderers can survive a killed browser.
leftover() {
    if pgrep -f -- "$app" >/dev/null 2>&1; then
        echo "FAIL leftover processes after $1:"; pgrep -af -- "$app" | head -5; fail=1
        pkill -f -- "$app" 2>/dev/null
    fi
}

run() { # name, expected-exit-zero, args...
    name=$1; want_zero=$2; shift 2
    timeout 300 "$app" "$@" >"$log/$name.log" 2>&1
    code=$?
    result=$(grep -E '^HARNESS result=' "$log/$name.log" | tail -1)
    if [ "$want_zero" = yes ]; then
        case "$result" in
            *result=pass*) [ "$code" -eq 0 ] && { echo "ok   $name (exit 0, $result)"; leftover "$name"; return; } ;;
        esac
        echo "FAIL $name (exit $code, ${result:-no result line}); log: $log/$name.log"; fail=1
    else
        if [ "$code" -ne 0 ]; then echo "ok   $name (exit $code as expected)"; else echo "FAIL $name exited 0, a failure must exit non-zero; log: $log/$name.log"; fail=1; fi
    fi
    leftover "$name"
}

for h in rpc quit tab engine; do run "$h-harness" yes "--$h-harness"; done
YAAC_HARNESS_FAIL_AFTER_CLOSE=1 run "harness-self-test" no --engine-harness

data=$(mktemp -d "${TMPDIR:-/tmp}/yaac-persist.XXXXXX")
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-write >"$log/persist-write.log" 2>&1 && \
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-verify >"$log/persist-verify.log" 2>&1
if [ $? -eq 0 ]; then echo "ok   persist write+verify ($(grep SELFTEST "$log/persist-verify.log" | tail -1))"; else echo "FAIL persist; logs in $log"; fail=1; fi
leftover persist
YAAC_DATA_DIR=$data timeout 30 "$app" --persist-write >/dev/null 2>&1
[ $? -eq 2 ] && echo "ok   persist-write on a non-empty directory exits 2" || { echo "FAIL persist-write on a non-empty directory must exit 2"; fail=1; }

[ $fail -eq 0 ] && echo "all harnesses passed" || echo "FAILED"
exit $fail
