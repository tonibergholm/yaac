#!/bin/sh
# Prints only the storage prefix of one cookie: v10, v11, none, or unknown.
#
#   cookie-store-check.sh <data-dir> <cookie-name>
#
# v11 means Chromium encrypted the cookie with a key from the OS keyring. v10 means the fixed key of
# the basic password store (effectively unencrypted at rest): a blocker for a release build.
#
# The Cookies database is copied first and only the first three bytes of encrypted_value are read.
# Nothing else is printed. <data-dir> is the YAAC_DATA_DIR of the run (its cef/ tree holds the
# profiles). The cookie must have been set on a page the app loaded.
set -eu
[ "$#" -eq 2 ] || { echo "usage: cookie-store-check.sh <data-dir> <cookie-name>" >&2; exit 2; }
data=$1
name=$2
tmp=$(mktemp -d "${TMPDIR:-/tmp}/yaac-cookie-check.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM
db=$(find "$data/cef" -name Cookies -type f 2>/dev/null | head -n 1 || true)
if [ -z "$db" ]; then
    echo none
    exit 0
fi
cp "$db" "$tmp/Cookies"
# A copy is read; the live file is never opened. Output is the prefix alone.
python3 -I -c '
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
row = con.execute(
    "SELECT substr(encrypted_value, 1, 3) FROM cookies WHERE name = ? LIMIT 1", (sys.argv[2],)
).fetchone()
if row is None or row[0] is None or len(row[0]) < 3:
    print("none" if row is None else "unknown")
else:
    prefix = row[0].decode("ascii", "replace")
    print(prefix if prefix in ("v10", "v11") else "unknown")
' "$tmp/Cookies" "$name" 2>/dev/null || echo unknown
