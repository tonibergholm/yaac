#!/bin/sh
# Builds yaac-app and yaac-app-helper and assembles target/bundle/yaac-app.app with the CEF
# framework and the helper apps. Ad-hoc signed.
#
#   scripts/bundle-macos.sh             debug bundle (dev tools, harness, YAAC_UI_DIR)
#   PROFILE=release scripts/bundle-macos.sh
#                                        release bundle: builds ui/dist first and embeds it; no
#                                        test machinery and no directory UI source.
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
profile=${PROFILE:-debug}

cd "$crate"
if [ "$profile" = release ]; then
  # The release binary embeds ui/dist, so the UI is built first. A failed UI build stops the bundle.
  (cd "$root/ui" && pnpm install --frozen-lockfile && pnpm build)
  cargo build --release -p yaac-app --bins
else
  cargo build -p yaac-app --bins
fi

rm -rf "$out"
"$bundler" yaac-app -o "$out" -i dev.yaac.app -d yaac
app="$out/yaac-app.app"
if [ "$profile" = release ]; then
  # The bundler builds debug binaries; put the release ones in place of them.
  cp "$root/target/release/yaac-app" "$app/Contents/MacOS/yaac-app"
  for helper in "$app/Contents/Frameworks/"*Helper*.app; do
    cp "$root/target/release/yaac-app-helper" "$helper/Contents/MacOS/$(basename "$helper" .app)"
  done
fi
codesign --force --deep -s - "$app"
echo "bundle: $app ($profile)"
