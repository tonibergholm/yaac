#!/bin/sh
# Builds yaac-app and yaac-app-helper and assembles target/bundle/yaac-app.app with the CEF
# framework and the helper apps. Ad-hoc signed.
#
# Needs the CEF distribution and the cef-rs bundler from the spike cache (see crates/yaac-app/README.md).
# Override the locations with the environment variables below.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
crate=$(cd "$here/.." && pwd)
root=$(cd "$crate/../.." && pwd)

cache=${YAAC_SPIKE_CACHE:-$HOME/Developer/yaac-spike-cache}
export CARGO_HOME=${CARGO_HOME:-$cache/cargo-home}
export CEF_PATH=${CEF_PATH:-$cache/cef}
export PATH=$cache/tools/py/bin:$cache/tools/py/cmake/data/bin:$PATH
bundler=${BUNDLER:-$cache/bundler/bin/bundle-cef-app}
out=${BUNDLE_OUT:-$root/target/bundle}

cd "$crate"
cargo build -p yaac-app --bins
rm -rf "$out"
"$bundler" yaac-app -o "$out" -i dev.yaac.app -d yaac
codesign --force --deep -s - "$out/yaac-app.app"
echo "bundle: $out/yaac-app.app"
