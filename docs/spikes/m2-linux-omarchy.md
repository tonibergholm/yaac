# M2 on Omarchy: the Linux port of the walking skeleton

Date: 2026-10-23. Machine: `toni@omarchy` (Arch, kernel 7.1.8, Intel Iris Plus, Mesa 26.2.5, Hyprland 0.56.2). Branch `m2/linux`, from `main` b32c1fd.

Labels: **VERIFIED** (seen in a run, file or command output), **INFERRED** (reasoning from verified facts), **OPEN** (needs a run that was not possible today).

## Status

The app builds and runs on Omarchy. The Linux paths, the Linux CEF host, the bundle, the Arch package metadata and the harness suite work on Chromium's headless Ozone platform, with no compositor and no display. The **M2 acceptance criterion is not met yet**: painting, window decorations, input, the GPU path and the Wayland platform choice on Hyprland are **OPEN**. They need a display run, which was not possible today (next section).

## Blocked: display runs and the owner's session

- The brief asked for a nested headless Hyprland (`AQ_BACKENDS=headless`). **VERIFIED:** Hyprland 0.56.2 does not read `AQ_BACKENDS` or `HYPRLAND_HEADLESS_ONLY`; a getenv trace of the binary shows neither is read. The DRM backend is mandatory and aborts without a session (`CBackend::create() failed`). No other headless compositor is installed (no weston, sway, cage, labwc, Xvfb).
- A workaround ran a nested Hyprland on the DRM backend (`LIBSEAT_BACKEND=noop`, internal panel disabled, a virtual output, a `bwrap` sandbox). It worked for a foot client and a Mesa Iris renderer. **Stopped, not used.** It shares the owner's GPU and input devices, and the owner did not approve that method. Its early runs opened `/dev/input/event7` (the internal keyboard) and kept a non-master fd on `card1`.
- Side effects that remain: ten Hyprland `SIGABRT` core dumps (14:54 to 15:00 EEST) and one `hyprland-update-screen` `SIGSEGV` (15:06) in systemd-coredump, all from these trials. Removing them needs sudo, which was not used (`coredumpctl list`). The owner's Hyprland (pid 1180) was not touched and kept running. Our runtime directory `/run/user/1000/yaac-ns` was removed.
- No window and no input were sent to the owner's session.

## Built and verified today

| Item | Result | Label |
|---|---|---|
| Rust | `rustc 1.98.1`, already installed through rustup under `~/.cargo` (the non-login PATH does not show it). Nothing installed. | VERIFIED |
| CEF | 154.0.34 linux64 **minimal** from the Spotify index. sha1 `15c91ad767097217b2e82ba8b933fe30b134be69` matches the index. Local sha256 `e87e2c76304372dd28b899ceb6efa4b353f52700594ddc358a8237ec07ec6492`, pinned in the PKGBUILD; `makepkg --verifysource` passes it. Laid out as `$CEF_PATH/154.0.34/cef_linux_x86_64`, flattened as download-cef does, with a hand-written `archive.json`. | VERIFIED |
| No download in the build | The `cef-dll-sys` build used the local tree. An unflattened tree failed on a missing `locales/`, which is why the layout is flat. | VERIFIED |
| Debug and bundle builds | No warnings. `NEEDED libcef.so`, `RUNPATH [$ORIGIN]`. `target/bundle-linux/` has 12 entries: the executable, `libcef.so`, `libvk_swiftshader.so`, `libvulkan.so.1`, `v8_context_snapshot.bin`, `vk_swiftshader_icd.json`, `.pak` files, `icudtl.dat`, `chrome-sandbox`, `locales/`. No `libEGL` or `libGLESv2` ship; both come from the system (libglvnd). `ldd` reports no missing library. | VERIFIED |
| Symlink launch | A symlink to the bundle executable resolves `libcef.so` through `$ORIGIN` (the real file's directory). Shown with an unknown-argument exit 2, which is reached after dynamic loading. | VERIFIED |
| Helper re-exec | `yaac-app --type=...` is handled first in `main`, before arguments and directories. A bare renderer with no browser parent hangs, as CEF expects, and leaves no process once killed. | VERIFIED |
| No display | Start with no display variables: the app chooses `--ozone-platform=x11`, fails with `Missing X server`, exits 1, leaves no process. | VERIFIED |
| Tests | CI equivalent on the default members: fmt, layering, clippy `-D warnings`, all tests pass (`yaac-platform` has 14: 7 new XDG tests and 7 existing). Clippy `-D warnings` for `yaac-cef` and `yaac-app` passes on Linux and on macOS (the macOS run caught one compile error, now fixed). | VERIFIED |
| UI | `pnpm install --frozen-lockfile`, `build`, `typecheck`, `lint`, `test` pass on Omarchy. `ui/dist` builds and passes the CSP check. | VERIFIED |
| PKGBUILD | `makepkg --printsrcinfo` succeeds. `makepkg --verifysource` passes the sha256 of the CEF archive. `depends` is the `ldd` closure of the bundle, mapped to packages with `pacman -Qo` (direct `NEEDED` plus libglvnd and vulkan-icd-loader for `dlopen`). `makepkg --nobuild` and `namcap` were not run: `namcap` is not installed and `package()` was not run. | VERIFIED (metadata and sources) |

## Harness results, headless Ozone (no compositor)

Command: `YAAC_OZONE=headless YAAC_TEST_HOME=<scratch> crates/yaac-app/scripts/run-harnesses.sh` on Omarchy, at commit `805a6df` (the final runner). No display variable is set. Every app run is isolated under `YAAC_TEST_HOME`.

| Stage | With `ui/dist` | Without `ui/dist` |
|---|---|---|
| rpc-harness | pass | pass |
| quit-harness | pass | pass |
| tab-harness | pass | pass |
| engine-harness | pass | pass |
| harness-self-test (must exit non-zero) | exit 1, as expected | exit 1, as expected |
| persist write and verify | pass (`HOST ui-assets source=dir`) | pass |
| persist write on a non-empty dir | exit 2 | exit 2 |

Both modes: 7 `ok` lines, 0 `FAIL`, 0 leftover-process failures (`run-with-dist.log` and `run-without-dist.log` on Omarchy).

Runner checks at the same commit (VERIFIED):
- **Another instance survives.** An unrelated headless instance (its own HOME) kept running through a full runner pass. The runner counted none of its processes and killed none.
- **Refusal.** With an instance using the run's own HOME alive, the runner exits 2 before any stage and names the processes.
- **Process environment is overwritten.** Chromium rewrites the environment area of its processes on Linux, the browser process included, so `HOME` in `/proc/PID/environ` is not a reliable test after startup. The runner therefore also counts processes of this bundle and this user that started at or after the stage began (start time in `/proc/PID/stat`), and it kills only processes that still carry this run's HOME. Both paths were exercised on a live probe instance.

Before the window-size fix below, the engine harness failed at the popup stage. The runs above are after that fix and after every reviewer fix in this branch.

What this mode does **not** establish: painting, decorations, input, GPU output, the Wayland platform choice, anything the compositor does.

## Engine findings on Linux

1. **Window size (fixed, found by the engine harness).** On the headless Ozone platform the top-level window stayed `1x1` while its root panel was laid out at the requested `1000x640`. The browser views got no height (`760x0`), the popup click at `(100,100)` missed, and the popup stage failed. Fix: `os::fit_window` sets the window's client size before `show()`. It is a no-op on macOS, so macOS behaviour is unchanged. After the fix the root panel is `992x604` inside the `1000x640` window and the popup stage passes. (VERIFIED on headless Ozone. The cause on Wayland is **OPEN**: the same 1x1 default may appear there.)
2. **Client-side frame on headless Ozone.** The root panel sits 8 px in from each side and 28 px down inside the window, so Views draws its own frame. On a Wayland session Hyprland answers `xdg-decoration` with server-side mode (protocol VERIFIED in E2). Whether the frame then disappears is **OPEN**.
3. **GPU process crash loop on headless.** ANGLE tries the X display, fails, and the GPU process exits and restarts (VERIFIED in the engine log). The harness stages still pass. The headless run is not a GPU run.
4. **Sandbox (VERIFIED in headless shell mode).** With `--ozone-platform=headless` and no `--no-sandbox` on the command line, the renderer-type processes (forked from zygotes) show `Seccomp: 2`, `NoNewPrivs: 1` and a nested PID namespace (three `NSpid` values). The browser, its zygote and its children use the user-namespace sandbox. Three zygotes carry `--no-zygote-sandbox` with `Seccomp: 0`. Which process type they serve (GPU, network or utility) is **OPEN**. The GPU process is absent in headless mode.
5. **Idle memory (VERIFIED, headless shell mode, one sidebar and one content view on `about:blank`, no GPU process).** 10 processes. Sum of PSS 283 MB; the browser process alone is 123 MB PSS (RSS 212 MB). Summed RSS (805 MB) double-counts shared pages and is not used. A real GPU process would add to this; **OPEN** until a display run.
6. **Stray data directory (VERIFIED, headless shell mode).** A normal run leaves no `cef_user_data/` and no `chrome_debug.log` in `$HOME`. The earlier stray files came from a renderer started by hand without a browser, which is not a normal run.
7. **Password store (OPEN).** The mock keychain on Linux is `--password-store=basic`, debug builds with `YAAC_MOCK_KEYCHAIN=1` only. Which OSCrypt backend a release run selects on Hyprland, with no session bus or with a live keyring, is **OPEN**. Without a session bus, libsecret cannot connect (`Failed to connect to the bus` in the logs).
8. **Chromium's own `--ozone-platform` handling (VERIFIED).** CEF reads the switch from the process arguments. An explicit `--ozone-platform` on the command line wins over the app's default. Only debug and dev-tools builds accept it as an argument (`yaac-app/src/main.rs`).

## Review

Two independent reviews (`code-reviewer`). The first, against `b32c1fd..HEAD`, blocked merge. The second, of the fix commits, found no blocking issue for `c2c9f95`. It asked for `grep -qxF`, which is applied. Its residual question (whether Chromium's process-title rewrite hides `HOME`) was confirmed by the probe, and the start-time fallback in `805a6df` answers it. Fixed after the first review:

- The runner inherited `DISPLAY` and `XAUTHORITY`, so a dead nested session could fall back to the owner's XWayland. Display runs now unset both and force `--ozone-platform=wayland`; the dead-bus and runtime paths point into the test home in headless mode.
- The leftover check used an unanchored `pkill -f`. It now matches the exact executable path of processes owned by this user (Linux), waits up to 10 s for zygotes to exit, and kills only the PIDs found. macOS keeps `pgrep`.
- `set_size` ran on macOS. It is now behind `os::fit_window` (no-op on macOS).
- `args()` could panic on non-UTF-8 arguments. The helper check and the argument parser now read raw OS arguments.
- Stale comments and the architecture sentence about the Ozone flag were corrected. `build()` runs cargo offline.

Not applied, with reasons: the first reviewer said the bundle ships `libEGL.so` and `libGLESv2.so`. It does not (bundle listing above, and the CEF tree has neither). `yaac-app-helper` stays built on Linux (harmless, removal is a separate change). `optdepends=gtk3` was not added: nothing in the app loads GTK.

## Still to do (needs the owner)

Not run: anything on a display. The owner must decide how display runs happen. Two options:

**A. Owner in his live session (he runs it himself, at the machine).** The laptop goes offline at 18:30, so nothing is driven over SSH. Steps, in a terminal inside his Hyprland session:

1. `cd ~/yaac-m2/yaac && git pull --ff-only origin m2/linux`
2. Build the bundle: `CEF_PATH=~/yaac-m2-cache/cef crates/yaac-app/scripts/bundle-linux.sh`
3. Run the one script and paste the report it prints (`~/yaac-m2-cache/live-report.txt`): `crates/yaac-app/scripts/live-session-check.sh`. It prints its plan first, opens the window, keeps it open for `LIVE_SECONDS` (default 60) so you can click a tab, asks two yes/no questions, closes the app it started, and checks that it is gone.
4. Release-store check (needs a release bundle, see item 8 below): `PROFILE=release CEF_PATH=~/yaac-m2-cache/cef crates/yaac-app/scripts/bundle-linux.sh`, then run the script again with `BUNDLE_DIR` pointing at the release bundle (the script prints a note if only the debug bundle exists).

Idle lock: the script does not toggle it. If the screensaver interrupts the run, pause it yourself and undo afterwards.

**B. Nested session.** A nested compositor needs an approved method. The DRM-backed workaround above is one candidate; it is not approved. Decide before anything else runs.

The run-harnesses display mode stays guarded on purpose. For option A the owner runs the app directly; that is not the harness.

## Packaging

`packaging/arch/PKGBUILD` (`yaac-git`): `prepare()` runs `cargo fetch --locked`, `pnpm install --frozen-lockfile` and lays out the CEF tree with `archive.json`. `build()` runs offline (`CARGO_NET_OFFLINE`), builds the UI and calls `bundle-linux.sh` with `YAAC_BUNDLE_NO_UI=1`. `package()` copies the bundle to `/usr/lib/yaac/` and links `/usr/bin/yaac-app`. The sandbox needs unprivileged user namespaces (VERIFIED on this kernel in E2); no setuid helper. `yaac.desktop` has `StartupWMClass=cef`, which is **OPEN** until a display run shows the Wayland app id.

## Files

- Code: `crates/yaac-platform/src/linux.rs`, `crates/yaac-cef/src/linux.rs`, `crates/yaac-cef/src/{lib,lifecycle,window,ui,macos}.rs`, `crates/yaac-app/src/main.rs`, `crates/yaac-app/build.rs`.
- Scripts: `crates/yaac-app/scripts/bundle-linux.sh`, `crates/yaac-app/scripts/run-harnesses.sh`.
- Package: `packaging/arch/{PKGBUILD,yaac.desktop,yaac.svg}` (the SVG is a placeholder icon).
- No public signature in `yaac-core`, `yaac-engine` or `yaac-api` changed.

## Review questions, answered by observation (headless Ozone, Omarchy, 17:20 to 17:40)

- **Q2, sandboxed processes and the leftover check: VERIFIED, not a problem here.** A headless shell run (debug bundle) had 11 processes with the bundle's executable. `ps` lists the same 11; the runner's `app_pids` returns the same 11. Every one, including the renderers with `Seccomp: 2`, is owned by `toni` in `/proc` (`stat`), so the `-O` check does not hide them. The root-owned case from the review did not occur for these processes (the `/proc/PID` owner is the user for non-dumpable children too, observed).
- **Q3, which process carried HOME: VERIFIED for one run, the earlier observation not reproduced.** In a 20 s headless run all 11 processes carried `HOME=` in `/proc/PID/environ`. In an earlier 15 s probe the browser process did not. The refusal run's two PIDs were not recorded, so which process it found is not known. The start-time fallback stays as the safety net.
- **Q4, `--type=bogus`: VERIFIED, it aborts.** With `ulimit -c 0` the process ends with SIGTRAP (exit 133, a fatal CEF check). `coredumpctl` records the event with `Storage: none`, so no dump file was kept. A fix is not in: a parent-process check would reject the sandboxed renderers, whose `getppid()` is 0 inside their PID namespace (INFERRED from the namespace layout, not tested). Remaining.
- **Q5, libEGL and libGLESv2: VERIFIED.** `pacman -Qo` gives `libglvnd 1.7.0-3` for both, and `vulkan-icd-loader` for libvulkan. The PKGBUILD comments stay as written.
- **Q6, `~/.pki` in the real home: VERIFIED for the RPC harness.** `/home/toni/.pki` was absent before and after a run with an isolated HOME, and the isolated home got none either. Other stages were not checked.

## PKGBUILD check (item 6)

- **prepare(): VERIFIED** by `makepkg -o -d` as an unprivileged user, in a temp copy, with a `pnpm` shim (npx pnpm@12) and no install. The CEF layout (`libcef.so`, `locales`, `LICENSE.txt`, `CREDITS.html`, `archive.json`) and the UI dependencies were produced. `cargo fetch` was not logged separately.
- **package(): VERIFIED under fakeroot** by running the PKGBUILD's own `package()` against the prebuilt release bundle. Everything is `root:root`, `chrome-sandbox` is `0755` and not setuid, the three licence files are under `/usr/share/licenses/yaac/`, and `/usr/bin/yaac-app` links to `/usr/lib/yaac/yaac-app`. `libcef.so` is 1.4 GB uncompressed.
- **build(): NOT run.** The release build needs more free RAM than this machine had (about 550 MB free, 6 GB available) and the owner's session needs it. Not verified: that the release build compiles inside `makepkg`.

## PR description (PR #14, `m2/linux`)

M2: the walking skeleton on Linux (Omarchy, Hyprland, Wayland). The Linux paths, the Linux CEF host (helpers are the same executable, Ozone chosen explicitly, sandbox on, the release password store explicit), the bundle, the harness runner and the Arch package. Verified only headless (Chromium's headless Ozone platform, no compositor): the harness suite passes with and without `ui/dist`, and the macOS suite passes on the Mac. Not verified: painting, decorations, input, the GPU path and the Wayland platform on Hyprland. Those need the owner at his machine (`scripts/live-session-check.sh`). Not done: see the list below.

## Remaining after the review (not done, in priority order)

- **Q4 fix:** `--type=<unknown>` aborts in CEF. A fix must not reject sandboxed children (see Q4).
- **Privacy, release store:** the explicit `gnome-libsecret` is in (coordinator). Still needed: the cookie check on a release bundle (`cookie-store-check.sh`, expect `v11`), run by the owner.
- **Final checks at `b9af4ff` (Omarchy, 17:45):** fmt, layering, clippy `-D warnings` (debug and release), 174 tests pass, headless harness suite passes with and without `ui/dist`. macOS at `fad1716`: suite all `ok`, self-test exit 1, `clippy --release -D warnings` passes.
- **Import regression caught:** the Ozone move (`c0068f0`) dropped a Linux-only import; the macOS build does not compile that module, so only the Linux check caught it (fixed in `b9af4ff`). Lesson: run the Linux `cargo check` after moving code out of a `cfg` module.
- **Live check:** `live-session-check.sh` has only run its dry run and its refusal outside a session.
- **Package build:** the release `makepkg` build was not run (RAM).

