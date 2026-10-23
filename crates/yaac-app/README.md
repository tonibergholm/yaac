# yaac-app: M1 phase 1 (engine and host, no core)

Status: the engine harness passes (`--engine-harness`, exit 0) and the RPC harness passes (`--rpc-harness`, exit 0), both run from the built `.app`. Shell mode opens a window and quits on Cmd-Q or an Apple Event with exit 0. Core wiring is phase 2.

## Commands

Build and bundle (needs the CEF 154.0.34 distribution and the cef-rs bundler in `~/Developer/yaac-spike-cache`, see `scripts/bundle-macos.sh`):

```sh
crates/yaac-app/scripts/bundle-macos.sh        # -> target/bundle/yaac-app.app (ad-hoc signed)
```

Run the harnesses (they print `ENGINE ...` and `HARNESS ...` lines; exit 0 = pass):

```sh
target/bundle/yaac-app.app/Contents/MacOS/yaac-app --engine-harness   # every stage
target/bundle/yaac-app.app/Contents/MacOS/yaac-app --rpc-harness      # RPC stage only
```

Shell mode (no flag): one window, sidebar and a content view. Use `open target/bundle/yaac-app.app`. `YAAC_UI_DIR` selects the UI build (default `ui/dist`); without a `sidebar.html` there, the placeholder in `harness/` is used. `YAAC_DATA_DIR` overrides the data directory.

Window screenshot used for the report: run the harness with `YAAC_HARNESS_HOLD_MS=8000`, read `native_window=` from the `browser created view=view-1` line, and run `screencapture -x -l <n> <file>` during the hold. Saved at `~/Developer/yaac-spike-results/m1-cef-harness.png` (sidebar shows `rpc ok`, content shows the popup host page).

## What the harness checks

`--engine-harness`, in order, each step failing the run on timeout or mismatch:

1. Views: create window, sidebar, two content views; `Created` for each; titles `Page A` and `Page B`.
2. Swap: `Show` the second view; the first stays alive (layout log: first `visible=false`, second `visible=true`, same bounds).
3. Navigation: `Load` to page C; `Navigated` then `TitleChanged` with the same `nav_gen`. `Back` to B, same check.
4. Hidden view: `Reload` the first view; it reports `Navigated`.
5. Popup: a trusted mouse click (`CefEngine::click`, a user gesture) on a `target=_blank` link; `PopupRequested{opener}` with `user_gesture: true`; the engine denies it.
6. Sidebar RPC: the sidebar POSTs on load; the stub handler sees `caller.view == sidebar, main_frame`; the page's title becomes `rpc-ok`.
7. Sidebar refusals: `probe.html` in the sidebar sends a `text/plain` POST and an OPTIONS request; expected `415` and `405`; the engine's rejection counter rises by 2.
8. Content refusals: a content view sends a `no-cors` POST to the RPC URL; the engine refuses it (rejection counter +1, `refused scheme caller=content`); a navigation of the content view to `app://ui/` is refused (`refused navigation`, `ERR_ABORTED`, no `Navigated` to `app:`).
9. Close: `Close` for each view; `Closed` for each; `WindowClosed` last; `quit(0)`.

## Scheme flags (the only place: `crates/yaac-cef/src/scheme.rs`, `scheme_option_bits`)

Shipped: `STANDARD | SECURE | FETCH_ENABLED`.

Evidence from the runs:
- With STANDARD and SECURE only (no `FETCH_ENABLED`, no CORS), the sidebar page stayed in `loading: true`, never fired a title change, and the RPC never reached the handler. With `FETCH_ENABLED` added, the same build loads the page, reaches the handler and completes the RPC. The cause of the stall was not traced further; `FETCH_ENABLED` is the flag that fixed it.
- `CORS_ENABLED` is not set. Without it the sidebar's same-origin `fetch`, POST and GET all worked (no preflight, same origin). A content view's CORS-mode `fetch` never reached the handler; the browser stopped it first. The content probe therefore uses `mode: 'no-cors'`, which does reach the handler and is refused there.
- `STANDARD` and `SECURE` were kept, not tested one at a time. Removing them is not checked.
- No CSP bypass flag is set. CSP is the UI's job (`'self'` relies on STANDARD).

Handler rules (`scheme.rs`): methods other than GET and POST get 405 (OPTIONS included, for every caller). Content and unattributed browsers get 403. Sidebar POST to `/api/v1/rpc` needs `Content-Type: application/json` (415 otherwise) and an `Origin` of `app://ui` when the header is present. No `Access-Control-*` headers are sent. Assets: `.js` is `text/javascript`, `.css` is `text/css`, `X-Content-Type-Options: nosniff` and `Cache-Control: no-store` on every response.

E-IPC-1 (a remote https page) is not run here. It uses the flag set above.

## CEF findings (observed in this run)

- `CefSettings.cache_path` must equal `root_cache_path`. An empty value gives incognito-only profiles; a named profile then fails.
- A profile cache path must be a direct child of the root (`CefSettings.root_cache_path`). CEF source: `chrome_browser_context.cc`, `cache_path_.DirName() == user_data_dir`. A nested `profiles/<id>` fails with "Cannot create profile at path".
- A named profile initializes asynchronously. `browser_view_create` returns null until then (`request_context_impl.cc`, `VerifyBrowserContext`). The engine retries `CreateView` on a 20 ms timer, up to about 5 s, and keeps later commands queued behind it. The C API has no readiness callback; this is a workaround.
- The scheme factory is registered on the profile context on every `CreateView` attempt, right before the view is created. I did not isolate whether registration before initialization is ignored; the CEF source suggests it, and this is unverified.
- cef-rs `PostData::elements` fills a vector sized to `element_count()`. An empty vector returns nothing, so the RPC body read was empty until this was fixed.
- CEF 154 calls the legacy `ResourceHandler::read`, not `read_response`. Both are implemented, sharing one body.
- Sidebar-to-core RPC: observed working end to end (POST, `Origin: app://ui`, JSON reply, title `rpc-ok`).

## Phase 2: where the core plugs in

- `crates/yaac-app/src/host.rs`, `start()`: replace the shell and harness arms with `spawn(store, engine)`. The engine is the `Arc<CefEngine>` that `run_browser` passes in.
- RPC: implement `yaac_cef::RpcHandler` for a wrapper over `CoreHandle`, calling `core.request(Principal::Ui { view: caller.view }, request)`. `UiCaller::main_frame` is there for policy commands. Replace `NoCore` in `main.rs` and `StubRpc` in `host.rs`.
- Events: `main.rs` creates `events_tx`/`events_rx`. Phase 2 forwards `events_rx` into `core.engine_events()` in a task spawned on the runtime.
- Pushes: `engine.push_ui_event(sidebar_view, &yaac_api::Event)` (a `subscribe()` forwarder).
- `CefMainThread` (`host.rs`) is the `yaac_platform::MainThread` hook; phase 2 passes it to the core.
- Persisted restart selftest: nothing in phase 1 depends on the store.

## Not done, or open

- No embedded UI build: `UiAssets` has `DirAssets` (dev, `YAAC_UI_DIR`) and `EmbeddedAssets`. rust-embed is not added, since `ui/dist` does not exist on this branch.
- E-IPC-1, A7 (cookie isolation between profiles), DevTools/PageDriver: not in phase 1.
- Popup handling is deny-only: the core opens tabs from `PopupRequested`.
- `use-mock-keychain` is passed to CEF as a dev-only switch until the SecretStore lands (`lifecycle.rs`).
- Only macOS arm64 is built. `yaac-cef` and `yaac-app` do not compile on other OSes (`compile_error!`). `yaac-platform` builds everywhere.
- Logs go to stdout; the CEF log file is `~/Library/Logs/yaac/cef.log`. No DiagnosticReports entry appeared for `yaac-app` in these runs.
