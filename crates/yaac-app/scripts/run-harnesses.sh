#!/bin/sh
# Runs every engine harness and the harness self-test against the built bundle, then the restart
# persistence check. Exits non-zero on the first failure. Run `scripts/bundle-macos.sh` first.
#
# Each harness must exit 0 AND print `HARNESS result=pass`. The self-test (a forced failure after the
# window closes) must exit non-zero, which shows the harness verdict reaches the exit code.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
app=${APP:-$root/target/bundle/yaac-app.app/Contents/MacOS/yaac-app}
export YAAC_MOCK_KEYCHAIN=1
fail=0
log=$(mktemp -d)

run() { # name, expected-exit-zero, args...
    name=$1; want_zero=$2; shift 2
    timeout 300 "$app" "$@" >"$log/$name.log" 2>&1
    code=$?
    result=$(grep -E '^HARNESS result=' "$log/$name.log" | tail -1)
    if [ "$want_zero" = yes ]; then
        case "$result" in
            *result=pass*) [ "$code" -eq 0 ] && { echo "ok   $name (exit 0, $result)"; return; } ;;
        esac
        echo "FAIL $name (exit $code, ${result:-no result line}); log: $log/$name.log"; fail=1
    else
        if [ "$code" -ne 0 ]; then echo "ok   $name (exit $code as expected)"; else echo "FAIL $name exited 0, a failure must exit non-zero; log: $log/$name.log"; fail=1; fi
    fi
}

for h in rpc quit tab engine; do run "$h-harness" yes "--$h-harness"; done
YAAC_HARNESS_FAIL_AFTER_CLOSE=1 run "harness-self-test" no --engine-harness

data=$(mktemp -d)
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-write >"$log/persist-write.log" 2>&1 && \
YAAC_DATA_DIR=$data timeout 90 "$app" --persist-verify >"$log/persist-verify.log" 2>&1
if [ $? -eq 0 ]; then echo "ok   persist write+verify ($(grep SELFTEST "$log/persist-verify.log" | tail -1))"; else echo "FAIL persist; logs in $log"; fail=1; fi
YAAC_DATA_DIR=$data timeout 30 "$app" --persist-write >/dev/null 2>&1
[ $? -eq 2 ] && echo "ok   persist-write on a non-empty directory exits 2" || { echo "FAIL persist-write on a non-empty directory must exit 2"; fail=1; }

[ $fail -eq 0 ] && echo "all harnesses passed" || echo "FAILED"
exit $fail
