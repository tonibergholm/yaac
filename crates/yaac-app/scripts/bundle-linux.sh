#!/bin/sh
# Builds yaac-app and assembles target/bundle-linux/: the executable with the CEF runtime next to
# it. The Linux counterpart of bundle-macos.sh. The CEF helper processes are this same executable
# (re-run with --type=...), so there is no helper binary to copy.
#
#   scripts/bundle-linux.sh                    debug bundle (dev tools, harness, YAAC_UI_DIR)
#   PROFILE=release scripts/bundle-linux.sh    release bundle: builds ui/dist and embeds it
#
# CEF_PATH is the CEF root. The distribution is expected at $CEF_PATH/154.0.34/cef_linux_x86_64
# (the minimal linux64 archive, extracted, with an archive.json), the same layout the build uses.
# Set CEF_DIST to point somewhere else. See crates/yaac-app/README.md.
#
# The bundle runs in place. packaging/arch/PKGBUILD installs the same layout under /usr/lib/yaac.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
crate=$(cd "$here/.." && pwd)
root=$(cd "$crate/../.." && pwd)

: "${CEF_PATH:?set CEF_PATH to the CEF root (see crates/yaac-app/README.md)}"
export CEF_PATH
cef_dist=${CEF_DIST:-$CEF_PATH/154.0.34/cef_linux_x86_64}
if [ ! -f "$cef_dist/libcef.so" ] || [ ! -f "$cef_dist/archive.json" ]; then
    echo "bundle-linux: no CEF distribution at $cef_dist (libcef.so and archive.json expected)" >&2
    echo "bundle-linux: the layout is the download-cef one: Release/ and Resources/ flattened" >&2
    exit 1
fi
out=${BUNDLE_OUT:-$root/target/bundle-linux}
# The bundle directory is deleted and rebuilt, so it must lie strictly under the repo's target/.
case "$out" in
    "$root/target/"*/*|"$root/target/"?*) ;;
    *) echo "bundle-linux: BUNDLE_OUT must be a directory under $root/target/ (got $out)" >&2; exit 2 ;;
esac
case "$out" in
    *'/../'*|*/..|*/.) echo "bundle-linux: BUNDLE_OUT must not contain '..' or '.'" >&2; exit 2 ;;
esac
profile=${PROFILE:-debug}

cd "$crate"
if [ "$profile" = release ]; then
    # The release binary embeds ui/dist, so the UI is built first. A failed UI build stops the bundle.
    # YAAC_BUNDLE_NO_UI=1: the caller already built ui/dist (the Arch package builds it in build()).
    if [ "${YAAC_BUNDLE_NO_UI:-0}" != 1 ]; then
        (cd "$root/ui" && pnpm install --frozen-lockfile && pnpm build)
    fi
    cargo build --release --locked -p yaac-app --bin yaac-app
    bin=$root/target/release/yaac-app
else
    cargo build --locked -p yaac-app --bin yaac-app
    bin=$root/target/debug/yaac-app
fi

rm -rf "$out"
mkdir -p "$out"
cp "$bin" "$out/yaac-app"
# The runtime files of the flattened CEF tree (download-cef moves Release/ and Resources/ up into
# the distribution directory): libcef.so and the other shared objects, the sandbox helper if
# present, the V8 snapshot, the .pak files, icudtl.dat and locales/. The source tree (include/,
# libcef_dll/, cmake/, build files) is not part of the runtime. The executable finds libcef.so
# through its RUNPATH ($ORIGIN). libEGL and libGLESv2 are not shipped: they come from the system
# (libglvnd), see the PKGBUILD.
for file in "$cef_dist"/*.so* "$cef_dist"/*.bin "$cef_dist"/*.dat "$cef_dist"/*.pak \
            "$cef_dist"/vk_swiftshader_icd.json "$cef_dist"/chrome-sandbox; do
    [ -f "$file" ] && cp -a "$file" "$out/"
done
cp -a "$cef_dist/locales" "$out/"
test -f "$out/libcef.so" || { echo "bundle-linux: libcef.so missing from the bundle" >&2; exit 1; }

# Fail here, not at launch, if a shared library cannot be resolved.
if ldd "$out/yaac-app" | grep -q "not found"; then
    ldd "$out/yaac-app" | grep "not found" >&2
    echo "bundle-linux: unresolved shared libraries (install the runtime packages, see PKGBUILD)" >&2
    exit 1
fi
echo "bundle: $out/yaac-app ($profile)"
