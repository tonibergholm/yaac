# yaac-app

The app binary: the real core, the SQLite store and the CEF engine on macOS. The engine harness,
the persistence checks and the local test site are dev-only (debug builds, or `--features dev-tools`).

## Status (progress notes)

- Quit and teardown: `WindowClosing` is sent first when a window starts closing; the engine enters
  a quitting state (commands dropped, RPCs get 503, no `Closed`, pending creations cancelled); the
  host's shutdown hook stops the core, waits up to 2 s for in-flight RPCs, and stops the runtime
  before `CefShutdown`. `post_to_ui` returns false once the shutdown gate is closed.
- Events: titles and URLs are capped (512 characters, 8 KiB); navigation, title and load state are
  coalesced per view on a 50 ms flush; lifecycle events are never coalesced or dropped; popups are
  denied always and reported at most 4 per second per opener.
- Commands run in order through one queue. A failed creation emits `CreateFailed`. A crashed
  browser is closed and leaves the layout.
- Persistence checks: `--persist-write` and `--persist-verify` need `YAAC_DATA_DIR`; write needs it
  empty. Both wait until one sidebar and one content view are live and one sidebar RPC was answered
  through the scheme before they run.
- Startup: the store is opened and the core spawned on a blocking thread. RPCs before the core is
  ready get `internal` with "the core is starting; retry".
- Release builds: `embed-ui` serves `ui/dist` compiled into the binary. No harness, no persistence
  checks, no `YAAC_UI_DIR`, no `click`. The release bundle path in `scripts/bundle-macos.sh` is
  written but not run here (see "Not verified").

## Commands

```sh
crates/yaac-app/scripts/bundle-macos.sh                 # debug bundle -> target/bundle/yaac-app.app
PROFILE=release crates/yaac-app/scripts/bundle-macos.sh # release bundle (builds ui/dist first)
```

Environment: `CEF_PATH=$HOME/Developer/yaac-spike-cache/cef`, the cmake and ninja directories of
the spike cache on `PATH`, `CARGO_HOME=$HOME/.cargo`. Debug builds read the mock keychain only with
`YAAC_MOCK_KEYCHAIN=1` (exactly `1`); without it they use the real Keychain.

```sh
B=target/bundle/yaac-app.app/Contents/MacOS/yaac-app
YAAC_MOCK_KEYCHAIN=1 $B --engine-harness      # all engine stages (exit 0 = pass)
YAAC_MOCK_KEYCHAIN=1 $B --rpc-harness         # sidebar RPC, refusals, remote probes, close
YAAC_MOCK_KEYCHAIN=1 $B --quit-harness        # requests quit right after creating a view
YAAC_DATA_DIR=$D $B --persist-write           # $D must be empty or missing
YAAC_DATA_DIR=$D $B --persist-verify          # same $D
$B                                            # the app; quit with Cmd-Q
```

Window screenshot: run `--engine-harness` with `YAAC_HARNESS_HOLD_MS=8000`, read `native_window=`
from the `browser created view=view-1` line, and run `screencapture -x -l <n> <file>` during the
hold (`~/Developer/yaac-spike-results/m1-cef-harness.png` was taken this way in the spike).

## Scheme flags (one place: `crates/yaac-cef/src/scheme.rs`, `scheme_option_bits`)

Shipped: `STANDARD | SECURE | FETCH_ENABLED`. CORS is not set.

- `FETCH_ENABLED` is needed: without it the sidebar page did not finish loading and its RPC never
  reached the handler (observed in the spike).
- `CORS_ENABLED` is not set. The same-origin sidebar requests work without it.
- Remote pages are refused without CORS (E-IPC-1 below).
- No CSP bypass flag is set. UI HTML gets the strict CSP header from the UI assets (dev directory
  or embedded), not the harness placeholder. Only the unit tests check the header; no real UI page
  was served in these runs.

## What the harness covers (observed)

`--engine-harness`, last run: exit 0.

- Views: created, `Created` and titles for two content views, swap by `Show` (layout log shows the
  hidden view `visible=false`), navigate, back, reload of the hidden view.
- Popup: a trusted click on a `target=_blank` link gives `PopupRequested` with `user_gesture: true`;
  the popup is denied.
- Title storm: a page changes its title 100,000 times. 6 title events reached the core; the final
  title is capped at 512 characters; RSS grew 52 MB (limit 200 MB); the sidebar RPC after the storm
  answered in 51 ms.
- A7 isolation (two profiles, cookies and localStorage, served from 127.0.0.1): the cookie set in
  profile A is absent from profile B and present in the second A view (`cookie_b=""`,
  `cookie_a2="yaac_probe=alpha"`). localStorage: `null` in B, `alpha` in A.
- Sidebar RPC answered through the scheme (`rpc-ok`); sidebar refusals: text/plain POST gets 415,
  OPTIONS gets 405, both counted (2 rejections).
- E-IPC-1 from a remote page in a content view (`http://127.0.0.1`), 23 attempts: fetch GET, POST
  text/plain, POST JSON, no-cors POST, credentials POST, XHR variants, form POST, anchor click,
  `window.open`, iframe to `app://ui/`, srcdoc, data:, javascript:, blob: frames that call RPC,
  302 redirect and meta refresh to `app://ui/`, and a final `location` change. Observed: 0 calls
  reached the RPC handler from content browsers; 11 rejections counted by the engine; no content
  view navigated into `app://`. Each attempt is printed as `E-IPC-1 attempt=<name> result=<outcome>`.
- Sidebar probes, one load per attempt: `location` to http, `window.open`, an iframe to http,
  `target=_blank` click, `location` to data: and javascript:. Observed: the sidebar never left
  `app://ui/` (`escaped=false` for all six). Two attempts did not finish their script: the iframe
  attempt and the `target=_blank` attempt (`completed=false`). That is a finding: the sidebar
  renderer stalls on those navigations. The next attempt reloads the page.
- Close: explicit `Close` per view, `Closed` per view, `WindowClosed` last. `HOST shutdown
  rpc_drained=true`, exit 0.

`--rpc-harness`: the same sidebar stages, exit 0 with the same two stalled probes.

`--quit-harness`: exit 0 in under a second. Limitation: the commands are still queued when the quit
lands, so this does not exercise a creation that is pending in the retry queue.

Persistence (fresh `YAAC_DATA_DIR`): `--persist-write` ok (4 nodes), `--persist-verify` ok (4 nodes,
selected tab restored), `--persist-write` on the non-empty directory exits 2.

Shell: launched directly with a scratch `YAAC_DATA_DIR`, quit by Apple Event: exit 0. No new file in
`~/Library/Logs/DiagnosticReports` for `yaac` during the runs.

## Not verified

- Closing the window (red button or Apple Event `close`): not automated. The Apple Event `close`
  verb did not reach the window in this environment, and the process stayed alive. A run killed by
  hand exited 0, which proves nothing; it is not counted.
- A creation pending in the retry queue cancelled by quit (see `--quit-harness` above).
- A crashed renderer closing its view (no test).
- The release bundle (`PROFILE=release`): not built, no `ui/dist` in this checkout. Release gating
  is checked by reading the cfg attributes, not by a release build.
- CSP header on a real UI page: unit tests only.
- E-IPC-1 against a remote https page (the spec allows it later, with the flags above).
- The final lint fixes (a closure replaced by a function path, a `format!` without arguments turned
  into a literal, an `if` collapsed into a let chain) were made after the last bundle run. They do
  not change behaviour, but the bundle was not rebuilt for them.

## Phase 2 notes

The core is wired in `src/host.rs` (`start_core`, `shutdown`), with `CoreRpc` as the RPC handler.
The engine events go to the core through `forward_engine_events`.
