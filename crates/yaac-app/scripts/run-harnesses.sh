#!/bin/sh
# Runs every engine harness and the harness self-test against the built bundle, then the restart
# persistence check. Exits non-zero on the first failure. Run `scripts/bundle-macos.sh` (macOS) or
# `scripts/bundle-linux.sh` (Linux) first.
#
# Each harness must exit 0 AND print `HARNESS result=pass`. The self-test (a forced failure after the
# window closes) must exit non-zero, which shows the harness verdict reaches the exit code.
#
# Linux has two modes. Both run the app with HOME, the XDG directories and TMPDIR under
# YAAC_TEST_HOME, so no app state lands in the owner's home.
#   - YAAC_OZONE=headless: Chromium's headless Ozone platform, no compositor, no display variables.
#     Checks the engine and the harness verdicts. Cannot reach any session.
#   - default: a display run, only inside a NESTED headless Hyprland session. `guard_linux` refuses
#     to start unless the environment is that session, and it removes DISPLAY and XAUTHORITY so a
#     dead nested session cannot fall back to the owner's XWayland. See docs/spikes/m2-linux-omarchy.md.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
os=$(uname -s)
export YAAC_MOCK_KEYCHAIN=1
fail=0
ozone_arg=

case "$os" in
    Darwin)
        app=${APP:-$root/target/bundle/yaac-app.app/Contents/MacOS/yaac-app}
        log=$(mktemp -d)
        ;;
    Linux)
        app=${APP:-$root/target/bundle-linux/yaac-app}
        : "${YAAC_TEST_HOME:?Linux: set YAAC_TEST_HOME to a scratch directory outside the real home data}"
        case "$YAAC_TEST_HOME" in
            /*) ;;
            *) echo "run-harnesses: YAAC_TEST_HOME must be an absolute path" >&2; exit 2 ;;
        esac
        ;;
    *) echo "run-harnesses: unsupported OS $os" >&2; exit 2 ;;
esac
if [ ! -x "$app" ]; then
    echo "run-harnesses: no executable at $app (run the bundle script first)" >&2
    exit 2
fi
app_real=$app
[ "$os" = Linux ] && app_real=$(readlink -f "$app")

# Points HOME, the XDG directories and TMPDIR at YAAC_TEST_HOME and creates them.
isolate_home() {
    export HOME=$YAAC_TEST_HOME/home
    export XDG_CONFIG_HOME=$YAAC_TEST_HOME/config
    export XDG_DATA_HOME=$YAAC_TEST_HOME/data
    export XDG_CACHE_HOME=$YAAC_TEST_HOME/cache
    export XDG_STATE_HOME=$YAAC_TEST_HOME/state
    export TMPDIR=$YAAC_TEST_HOME/tmp
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$TMPDIR"
    log=$(mktemp -d "$TMPDIR/harness-logs.XXXXXX")
}

# A display run: the owner's session must never receive a window or input from it. Refuse unless the
# environment is the nested headless session (XDG_RUNTIME_DIR, its Wayland socket, its Hyprland
# instance and its dead bus). No X display is passed, so Chromium cannot choose XWayland.
guard_linux() {
    case "${XDG_RUNTIME_DIR:-}" in
        /*/yaac-nested) ;;
        *) echo "run-harnesses: XDG_RUNTIME_DIR must be the absolute nested session directory (*/yaac-nested); the owner's runtime directory is never used" >&2; exit 2 ;;
    esac
    # Names come from the environment: a path separator or a link could point outside the nested session.
    case "${WAYLAND_DISPLAY:-}" in
        ''|*/*) echo "run-harnesses: WAYLAND_DISPLAY must be a plain socket name" >&2; exit 2 ;;
    esac
    case "${HYPRLAND_INSTANCE_SIGNATURE:-}" in
        ''|*/*) echo "run-harnesses: HYPRLAND_INSTANCE_SIGNATURE must be a plain name" >&2; exit 2 ;;
    esac
    [ -d "$XDG_RUNTIME_DIR" ] && [ ! -L "$XDG_RUNTIME_DIR" ] || { echo "run-harnesses: $XDG_RUNTIME_DIR is not a directory" >&2; exit 2; }
    [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] && [ ! -L "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] || {
        echo "run-harnesses: WAYLAND_DISPLAY is not the nested socket (or it is a link)" >&2; exit 2; }
    [ -d "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE" ] && [ ! -L "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE" ] || {
        echo "run-harnesses: HYPRLAND_INSTANCE_SIGNATURE is not the nested instance (or it is a link)" >&2; exit 2; }
    case "${DBUS_SESSION_BUS_ADDRESS:-}" in
        *yaac-nested/no-bus) ;;
        *) echo "run-harnesses: DBUS_SESSION_BUS_ADDRESS must be the dead nested bus" >&2; exit 2 ;;
    esac
    unset DISPLAY XAUTHORITY WAYLAND_SOCKET
    ozone_arg=--ozone-platform=wayland
    isolate_home
}

if [ "$os" = Linux ]; then
    if [ "${YAAC_OZONE:-}" = headless ]; then
        # No compositor at all. Every display variable is removed, and the runtime directory and the
        # bus point at dead paths inside the test home, so nothing can attach to a session.
        unset WAYLAND_DISPLAY DISPLAY XAUTHORITY WAYLAND_SOCKET HYPRLAND_INSTANCE_SIGNATURE
        isolate_home
        export XDG_RUNTIME_DIR=$YAAC_TEST_HOME/runtime
        mkdir -p -m 700 "$XDG_RUNTIME_DIR"
        export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/no-bus
        ozone_arg=--ozone-platform=headless
        echo "run-harnesses: headless Ozone mode (no compositor)"
    else
        guard_linux
    fi
fi

# Ticks (1/100 s) since boot, now. A stage records it before it starts a process.
now_ticks() { [ "$os" = Linux ] && awk '{ print int($1 * 100) }' /proc/uptime || echo 0; }

# Start time of a process, in ticks since boot (field 22 of /proc/PID/stat).
start_ticks() { sed 's/.*) //' "/proc/$1/stat" 2>/dev/null | awk '{ print $20 }'; }

# This run's HOME in the process environment. Chromium overwrites the environment area of its
# processes on Linux (observed on the browser process too), so this is only a first test.
ours() { tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -qxF "HOME=$HOME"; }

# PIDs of this bundle's processes owned by this user. The browser, zygotes, renderers and GPU
# process run the same executable. Linux matches the exact executable path and the owner uid, and
# counts a process whose environment has this run's HOME, or, when SINCE (ticks) is given, one
# that started at or after SINCE. A process from before the stage (another instance) is not counted
# by start time. macOS: pgrep for this user on the bundle path.
app_pids() {
    since=${1:-}
    if [ "$os" = Linux ]; then
        for d in /proc/[0-9]*; do
            [ -O "$d" ] || continue
            [ "$(readlink "$d/exe" 2>/dev/null)" = "$app_real" ] || continue
            p=${d#/proc/}
            if ours "$p"; then
                echo "$p"
            elif [ -n "$since" ] && [ "$(start_ticks "$p")" -ge "$since" ] 2>/dev/null; then
                echo "$p"
            fi
        done
    else
        pgrep -u "$(id -u)" -f -- "$app" 2>/dev/null
    fi
}

# Only processes that carry this run's HOME are killed (Linux). Others are reported and left alone.
killable() { [ "$os" != Linux ] || ours "$1"; }

# A run must not start while a process of this bundle with this HOME is still alive: its leftovers
# would be counted as this run's.
if [ "$os" = Linux ] && [ -n "$(app_pids)" ]; then
    echo "run-harnesses: processes of this bundle with this HOME are already running:" >&2
    app_pids >&2
    exit 2
fi

# Ticks at the start of the current stage (set by run() and the persistence checks).
stage_since=0

# No process of this bundle may outlive a stage: zygotes and renderers can exit a moment after the
# browser. Wait up to 10 s, then fail. Kill only the processes that carry this run's HOME.
leftover() {
    # macOS: no leftover check, as on main. Its pgrep pattern is not safe to act on.
    [ "$os" = Linux ] || return 0
    i=0
    while [ -n "$(app_pids "$stage_since")" ] && [ "$i" -lt 10 ]; do sleep 1; i=$((i + 1)); done
    pids=$(app_pids "$stage_since")
    if [ -n "$pids" ]; then
        echo "FAIL leftover processes after $1: $(echo $pids)"
        fail=1
        for p in $pids; do
            if killable "$p"; then kill -9 "$p" 2>/dev/null; fi
        done
    fi
}

run() { # name, expected-exit-zero, args...
    name=$1; want_zero=$2; shift 2
    stage_since=$(now_ticks)
    timeout 300 "$app" "$@" $ozone_arg >"$log/$name.log" 2>&1
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
stage_since=$(now_ticks)
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-write $ozone_arg >"$log/persist-write.log" 2>&1 && \
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-verify $ozone_arg >"$log/persist-verify.log" 2>&1
if [ $? -eq 0 ]; then echo "ok   persist write+verify ($(grep SELFTEST "$log/persist-verify.log" | tail -1))"; else echo "FAIL persist; logs in $log"; fail=1; fi
leftover persist
YAAC_DATA_DIR=$data timeout 30 "$app" --persist-write $ozone_arg >/dev/null 2>&1
[ $? -eq 2 ] && echo "ok   persist-write on a non-empty directory exits 2" || { echo "FAIL persist-write on a non-empty directory must exit 2"; fail=1; }

[ $fail -eq 0 ] && echo "all harnesses passed" || echo "FAILED"
exit $fail
