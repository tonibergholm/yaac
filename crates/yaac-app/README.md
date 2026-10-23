# yaac-app

The app binary: the real core, SQLite store and CEF engine on macOS. The engine harness, the
persistence checks and the local test site exist only in debug and `dev-tools` builds.

## Status (second review round, branch m1/fix2-cef)

Done and verified:
- Persistence: `--persist-write` then `--persist-verify` on one fresh `YAAC_DATA_DIR`, both exit 0,
  with `ui/dist` present. The write refuses a non-empty directory (exit 2).
- Shell launch quit by Apple Event: exit 0, with `ui/dist` present.
- Quit during pending creation (`--quit-harness`): passes with a test hook that holds profiles not
  ready. `CreateFailed` is reported for both views, `WindowClosing` comes first and `WindowClosed` last.
  Limitation: the harness sets the pass code before the close, because the loop can end as soon as
  `WindowClosed` is sent. A harness still running at that point cannot fail the run.
- Unit tests: yaac-cef 32, yaac-platform 8. Clippy `-D warnings`, fmt and the layering check are clean.
- Private directories: one fd-based check in yaac-platform (O_NOFOLLOW leaf, fstat owner, fchmod 0700,
  symlinks refused below HOME/TMPDIR before anything is created), used by the engine too.
- Event flushing before core shutdown, release build without `embed-ui` fails with a clear message,
  popup budgets per gesture, dev-only per-event logs, crash close posted to the UI thread.
- Remote-page probes extended (service worker, named window.open, rel=noopener, prerender, ping,
  Link preload, WebSocket, Request navigate, postMessage). The harness requires a report for each
  name in `REMOTE_PROBES`. Last run: 33 attempts, 0 content calls reached the RPC handler, 12
  engine rejections, no navigation into `app://`.

Not done or failing:
- Blocking 1, the cancelled window close. The close now waits for the beforeunload answer
  (`unload_answered`), but the cancelled-close stage still fails. An unexpected window close starts
  right after the unload page loads, before the harness clicks or sends its close. The stay answer
  therefore arrives too late to be cancelled. The cause is not found.
  `--engine-harness` exits 1 for this reason; `--rpc-harness` exits 1 because the final close stage
  commits early and drops the last `Close` with `reason=quitting`. The same early-close family.
- Item 6 (NavTracker, real callback traces as fixtures): not done.
- Item 8 bounded non-blocking log channel: not done (dev-only logs and summarised popups only).
- Item 4 (Cmd-Q during startup) and item 5 (crash test): code checks added, no test run.
- Items 10 unit tests for the owner check: skipped (root only).
- Release bundle (`PROFILE=release`): not built and run here.
- Run without `ui/dist`: not run in this round.

## Linux (M2, Omarchy)

Same app, Linux build. The CEF helper processes are this executable re-run with `--type=...`, and
`main` handles them first. Layout and status: `docs/spikes/m2-linux-omarchy.md`.

```sh
# CEF: the minimal linux64 distribution of 154.0.34, extracted and flattened as download-cef does
# (Release/ and Resources/ moved up), with an archive.json, at
#   $CEF_PATH/154.0.34/cef_linux_x86_64
CEF_PATH=$HOME/yaac-m2-cache/cef crates/yaac-app/scripts/bundle-linux.sh   # target/bundle-linux/
YAAC_OZONE=headless YAAC_TEST_HOME=$HOME/yaac-m2-cache/run \
    crates/yaac-app/scripts/run-harnesses.sh                                # no compositor
```

`YAAC_OZONE=headless` runs every stage on Chromium's headless Ozone platform with no display
variables set. It checks the engine and the harness verdicts. It does not check painting, window
decorations or input. Display runs need a nested headless session; the runner refuses to start
outside one. The Ozone platform is chosen by the app: `wayland` when the Wayland socket is reachable,
`x11` otherwise (lowercase, always set). Development builds accept `--ozone-platform=<value>` to
override it.

Packaging: `packaging/arch/PKGBUILD` (`yaac-git`). Sandbox: user namespaces, no setuid helper.

## Run everything

`crates/yaac-app/scripts/bundle-macos.sh` then `crates/yaac-app/scripts/run-harnesses.sh` runs the four harnesses, the harness self-test (a forced failure after the window closes must exit non-zero) and the restart persistence check. It exits non-zero on any failure.

## Commands

```sh
crates/yaac-app/scripts/bundle-macos.sh                 # debug bundle -> target/bundle/yaac-app.app
PROFILE=release crates/yaac-app/scripts/bundle-macos.sh # release bundle (builds ui/dist first)
```

Environment: `CEF_PATH=$HOME/Developer/yaac-spike-cache/cef`, the cmake and ninja directories of the
spike cache on `PATH`, `CARGO_HOME=$HOME/.cargo`. Debug builds read the mock keychain only with
`YAAC_MOCK_KEYCHAIN=1` (exactly `1`).

```sh
B=target/bundle/yaac-app.app/Contents/MacOS/yaac-app
YAAC_MOCK_KEYCHAIN=1 $B --engine-harness   # exit 0 = pass (currently 1: cancelled-close stage)
YAAC_MOCK_KEYCHAIN=1 $B --rpc-harness      # exit 0 = pass (currently 1, see above)
YAAC_MOCK_KEYCHAIN=1 $B --quit-harness     # exit 0 = pass
YAAC_DATA_DIR=$D $B --persist-write        # $D empty or missing
YAAC_DATA_DIR=$D $B --persist-verify       # same $D
$B                                         # the app; quit with Cmd-Q
```

## Scheme flags (one place: `crates/yaac-cef/src/scheme.rs`)

Shipped: `STANDARD | SECURE | FETCH_ENABLED`. CORS is not set. Remote pages are refused without CORS
(E-IPC-1). No CSP bypass flag.

## Known limit: partial closes

A window close asks content browsers first, then the sidebar. If one content tab's confirmation is
cancelled after another tab without a confirmation has already closed, the closed tab is reported as
closed and the window stays open. The production policy accepts every unload prompt, so this only
occurs in the harness's cancel test.
