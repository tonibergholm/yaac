#!/bin/sh
# live-session-check.sh: run by the owner in a terminal INSIDE his own Hyprland session on Omarchy.
#
#   crates/yaac-app/scripts/live-session-check.sh            # plan, then the run
#   crates/yaac-app/scripts/live-session-check.sh --dry-run  # print the plan only, touch nothing
#
# It starts the bundle's yaac-app with its data under ~/yaac-m2-cache/run-live, waits for the yaac
# window, records non-private facts in ~/yaac-m2-cache/live-report.txt, asks two questions, and
# closes the app it started. It never reads other windows, tab titles, URLs or browser profiles.
#
# Environment (all optional):
#   YAAC_REPO       checkout (default ~/yaac-m2/yaac)
#   LIVE_SECONDS    how long the window stays open (default 60)
#   LIVE_PORT       loopback port for the cookie test page (default 8765)
#   BUNDLE_DIR      bundle to run (default $YAAC_REPO/target/bundle-linux; use a release bundle for the store check)
set -u

dry=0
[ "${1:-}" = --dry-run ] && dry=1
repo=${YAAC_REPO:-$HOME/yaac-m2/yaac}
bundle=${BUNDLE_DIR:-$repo/target/bundle-linux}
run=$HOME/yaac-m2-cache/run-live
report=$HOME/yaac-m2-cache/live-report.txt
secs=${LIVE_SECONDS:-60}
port=${LIVE_PORT:-8765}
here=$(cd "$(dirname "$0")" && pwd)
app_pid=
http_pid=

say() { printf '%s\n' "$*"; }
note() { printf '%s\n' "$*" >> "$report"; }

# The plan is printed before anything runs.
say "live-session-check: this is the plan. Nothing is done before it is printed."
say "  1. check that this terminal is inside a Wayland session (WAYLAND_DISPLAY, Hyprland signature)"
say "  2. use the bundle $bundle (report which profile it was built with)"
say "  3. start the bundle's yaac-app with HOME and XDG_* under $run (not your real home)"
say "  4. wait up to 30 s for the yaac window (matched by the process ID of the app started here)"
say "  5. record: window class, xwayland, floating, pinned, fullscreen, size; Ozone choice from the app log;"
say "     the sandbox state of one renderer; GPU process present; the cookie prefix"
say "  6. a window-only screenshot of the yaac window geometry to $HOME/yaac-m2-cache/live-window.png"
say "  7. keep the window open for $secs s (click a tab in it to check it)"
say "  8. ask two yes/no questions and record the answers"
say "  9. close the app with SIGTERM to the process ID started here, and check it is gone"
say "  Idle lock: this script does NOT toggle it. Pause it yourself if it interrupts the run."
say "  Report: $report (no titles, no URLs, no other windows)"
[ "$dry" = 1 ] && { say "dry run: stopping here."; exit 0; }

# 1. The owner's session. The script refuses to run outside one.
if [ -z "${WAYLAND_DISPLAY:-}" ] || [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || ! command -v hyprctl >/dev/null 2>&1; then
    say "This must run in a terminal inside the Hyprland session (WAYLAND_DISPLAY and HYPRLAND_INSTANCE_SIGNATURE are needed)."
    exit 2
fi
[ -x "$bundle/yaac-app" ] || { say "No bundle at $bundle. Build it first: CEF_PATH=~/yaac-m2-cache/cef crates/yaac-app/scripts/bundle-linux.sh"; exit 2; }
profile=$(cat "$bundle/BUILD-PROFILE" 2>/dev/null || echo unknown)

mkdir -p "$run/home" "$run/config" "$run/data" "$run/cache" "$run/state" "$run/tmp" "$HOME/yaac-m2-cache"
: > "$report"
note "yaac live-session check, $(date '+%Y-%m-%d %H:%M')"
note "bundle profile: $profile"

# 3. Start the app. Only the environment is changed; the session variables stay.
env HOME="$run/home" XDG_CONFIG_HOME="$run/config" XDG_DATA_HOME="$run/data" \
    XDG_CACHE_HOME="$run/cache" XDG_STATE_HOME="$run/state" TMPDIR="$run/tmp" \
    "$bundle/yaac-app" > "$run/app.log" 2>&1 &
app_pid=$!
say "started yaac-app with process ID $app_pid (log: $run/app.log)"

cleanup() {
    [ -n "$http_pid" ] && kill -TERM "$http_pid" 2>/dev/null
    if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then kill -TERM "$app_pid" 2>/dev/null; fi
}
trap cleanup EXIT INT TERM

# 4. Wait for the window of exactly this process.
window_json=
i=0
while [ "$i" -lt 30 ]; do
    window_json=$(hyprctl clients -j 2>/dev/null | python3 -I -c '
import json, sys
pid = int(sys.argv[1])
for c in json.load(sys.stdin):
    if c.get("pid") == pid:
        print(json.dumps(c))
        break
' "$app_pid")
    [ -n "$window_json" ] && break
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 1; i=$((i + 1))
done
if [ -z "$window_json" ]; then
    note "window: none found for this process within 30 s"
    say "No window for process $app_pid. See $run/app.log. Report written anyway."
else
    note "window (only the fields listed; title and address are never recorded):"
    printf '%s' "$window_json" | python3 -I -c '
import json, sys
c = json.load(sys.stdin)
for key in ("class", "initialClass", "xwayland", "floating", "pinned", "fullscreen", "mapped", "size", "at"):
    print("  " + key + ": " + json.dumps(c.get(key)))
' >> "$report"
fi

# 5. Facts from the process and its log.
note "ozone platform: $(grep -o 'yaac: ozone platform [a-z0-9]*' "$run/app.log" | tail -n 1 | sed 's/yaac: ozone platform //')"
renderer=
for d in /proc/[0-9]*; do
    [ -O "$d" ] || continue
    [ "$(readlink "$d/exe" 2>/dev/null)" = "$(readlink -f "$bundle/yaac-app")" ] || continue
    if tr '\0' ' ' < "$d/cmdline" 2>/dev/null | grep -q -- '--type=renderer'; then
        renderer=${d#/proc/}
        break
    fi
done
if [ -n "$renderer" ]; then
    note "renderer sandbox: $(grep -E '^(Seccomp|NoNewPrivs):' "/proc/$renderer/status" | tr -s ' \t' ' ' | tr '\n' ' ')"
else
    note "renderer sandbox: no renderer process found (open the window and try again)"
fi
if tr '\0' ' ' < /proc/*/cmdline 2>/dev/null | grep -q -- '--type=gpu-process'; then
    note "GPU process: present (chrome://gpu was not read; that needs a look at the screen)"
else
    note "GPU process: not present"
fi

# 6. Window-only screenshot, cropped to the yaac window.
if [ -n "$window_json" ]; then
    geom=$(printf '%s' "$window_json" | python3 -I -c '
import json, sys
c = json.load(sys.stdin)
x, y = c["at"]; w, h = c["size"]
print("%d,%d %dx%d" % (x, y, w, h))
')
    if grim -g "$geom" "$HOME/yaac-m2-cache/live-window.png" 2>/dev/null; then
        note "screenshot: $HOME/yaac-m2-cache/live-window.png (window region only)"
    else
        note "screenshot: grim failed"
    fi
fi

# Cookie test page on loopback. Open it in the yaac window's address bar while the window is open.
page=$run/www
mkdir -p "$page"
cat > "$page/cookie.html" <<'EOF'
<!doctype html><title>cookie check</title><script>document.cookie="yaac_check=1; path=/; max-age=3600";</script>yaac cookie check
EOF
python3 -I -m http.server --bind 127.0.0.1 --directory "$page" "$port" > "$run/http.log" 2>&1 &
http_pid=$!
say ""
say "Open this page in the yaac window (address bar): http://127.0.0.1:$port/cookie.html"
say "Click a tab in the sidebar and look at both areas. The window stays open for $secs s."
sleep "$secs"

# 7. Two questions only you can answer.
say ""
printf 'Do the sidebar AND the content area both show their contents? [y/n] '
read -r q1 || q1=
printf 'Did a titlebar appear on the yaac window? [y/n] '
read -r q2 || q2=
note "owner answer: both areas show content: ${q1:-no answer}"
note "owner answer: titlebar appeared: ${q2:-no answer}"

# Cookie prefix, from the profile of this run.
prefix=$("$here/cookie-store-check.sh" "$run/data/yaac" yaac_check 2>/dev/null || echo unknown)
note "cookie prefix (yaac_check, after opening the test page): $prefix"
if [ "$profile" != release ]; then
    note "note: this is a $profile bundle; the release bundle is the one that counts for the store check"
fi

# 9. Close the app started here, and check it is gone.
kill -TERM "$app_pid" 2>/dev/null
i=0
while kill -0 "$app_pid" 2>/dev/null && [ "$i" -lt 10 ]; do sleep 1; i=$((i + 1)); done
if kill -0 "$app_pid" 2>/dev/null; then
    note "app: still running after SIGTERM (process $app_pid)"
else
    wait "$app_pid" 2>/dev/null
    note "app: closed (process $app_pid is gone)"
fi
app_pid=
say ""
say "Report: $report"
cat "$report"
