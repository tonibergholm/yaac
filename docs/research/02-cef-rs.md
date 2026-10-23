# 02. Is cef-rs usable for a Rust-driven CEF host on macOS and Linux?

Accessed: 2026-10-23. Read-only research. No project code written, no builds run. Fetched scripts were not run.

Pinned sources (commit or version, so the citations stay stable):

- cef-rs: `tauri-apps/cef-rs` default branch `dev`, commit `fe2971b855c650b2a1b403b7c3f3521100e8b8fc` (2026-10-23T04:51Z). Local shallow clone: `/tmp/cefrs-UJtIMg/src`. Permalinks below use this commit.
- CEF: `chromiumembedded/cef` master, commit `7f8a5ec28e932aa62b583c8248e3df2e4dfe4807` (2026-10-20). Headers read from `raw.githubusercontent.com` (same master). Matches the pin in `01-cef-on-helium.md`.
- crates.io API (`/api/v1/crates/...`), CEF build index `https://cef-builds.spotifycdn.com/index.json` (10.5 MB, downloaded 2026-10-23).

Labels: **VERIFIED** = read in a primary source (file, API response, or index). **INFERRED** = my reasoning from verified facts. **UNKNOWN** = not found, with the experiment that would settle it.

Not opened, search snippets only: Level1Techs forum thread relaying the CEF maintainer (`https://forum.level1techs.com/t/are-we-wayland-yet/213781/91`) and the Collabora CEF GitLab readme. Both are labelled SECONDHAND where used.

---

## Summary

- cef-rs is the de facto Rust binding layer for CEF. It is actively maintained (commits on 2026-10-23), published 154.5.0+154.0.34 on 2026-10-07, and lives under the `tauri-apps` org. Five other Rust CEF crates on crates.io depend on `cef`/`cef-dll-sys` (Section 1.5).
- It exposes the full CEF C API, including Views (CefWindow, CefBrowserView, CefPanel, BoxLayout), all handler traits, and a port of CefMessageRouter. Its own examples use Views by default (VERIFIED).
- Platform coverage is declared for macOS and Linux on x86_64 and arm64, and bindings exist for all four. CI only builds and runs unit tests on ubuntu-latest and macos-latest. It never launches a browser, never bundles, and never tests Linux arm64, macOS x86_64, or Wayland (VERIFIED from workflow).
- Windowing recommendation: CEF Views. Native NSView or X11 child parenting is not a good fit. The cef-rs API types the Linux parent handle as an X11 window ID, so Wayland parenting is not possible through it (VERIFIED type). CEF's own cefclient runs a second BrowserView inside one top-level window, with a mixed runtime style (VERIFIED code).
- Verdict: go with caveats. Build the prototype on cef-rs with Views, and treat Wayland, the macOS NSApplication/message-loop setup, and the two-view mixed-style layout as the first things to prove.

---

## 1. Maturity

### 1.1 Crates and versions (VERIFIED, crates.io API, 2026-10-23)

| Crate | Latest | Published | Created | Downloads |
|---|---|---|---|---|
| `cef` | 154.5.0+154.0.34 | 2026-10-07 | 2023-05-22 (crates.io `created_at`, cause UNKNOWN) | 310,260 |
| `cef-dll-sys` | 154.5.0+154.0.34 | 2026-10-07 | 2025-01-18 | 289,855 |
| `download-cef` | 3.0.0 | 2026-09-12 | 2025-01-17 | 286,587 |
| `export-cef-dir` | 154.5.0+154.0.34 | 2026-10-07 | — | — |

Sources: `https://crates.io/api/v1/crates/cef`, `.../cef-dll-sys`, `.../download-cef`. The repo was created 2025-01-10 (`gh api repos/tauri-apps/cef-rs`).

Version scheme: the crate version is `<crate-semver>+<CEF version>`. The first number tracks the CEF major (e.g. 154), and the build metadata is the exact CEF version (`154.0.34`). Workspace version and the `cef-dll-sys` pin are in `Cargo.toml` L1-L40 (VERIFIED, `Cargo.toml`, `[workspace.package] version = "154.5.0+154.0.34"`).

Release cadence (crates.io version history):

- `cef`: 151.8.1 (2026-09-03), 152.0.0 (09-07), 152.1–152.4 (09-10 to 09-21), 154.0.0 (09-23), 154.1.0 (09-26), 154.2.0 (09-27), 154.3.0 (09-30), 154.4.0 (10-03), 154.5.0 (10-07). Roughly one release every 2–4 days in September–October 2026.
- Each release tags three GitHub releases (`cef-v…`, `cef-dll-sys-v…`, `export-cef-dir-v…`) (VERIFIED, `gh api repos/tauri-apps/cef-rs/releases`).

### 1.2 Repo health (VERIFIED, `gh api`, 2026-10-23)

- Stars 482. License Apache-2.0 at repo level; crates are `Apache-2.0 OR MIT` (`Cargo.toml` `[workspace.package]`). Default branch `dev`. Not archived.
- Open issues 8, closed issues 59. Open PRs 7, merged PRs 343 (search API counts).
- Contributors (API, top 10): `wravery` 590 commits, `github-actions[bot]` 206, `csmoe` 125, `renovate[bot]` 59, `rgon` 31, `lucasfernog` 25, `dependabot[bot]` 20, `tasuren` 16, `wusyong` 13, `FabianLars` 6. Cargo authors: Wu Yu Wei, Bill Avery, csmoe. `lucasfernog`, `FabianLars`, and `wusyong` are the contributors I would check first for Tauri ties (affiliation INFERRED, not verified from a source).
- Commit cadence: last eight commits span 2026-09-26 to 2026-10-23. Most are `chore: update bindings (#486–#492)`, one every 3–4 days. Latest `fix(osr): copy shared textures before accelerated paint callbacks return (#498)` on 2026-10-23 (`gh api repos/tauri-apps/cef-rs/commits`).
- Issue #297 (`wrap_* macros are not good DX`, open) and #417 (OSR VRAM leak on AMD GPU, open) are the open issues most relevant to us.

### 1.3 Tracking CEF versions

- CEF index, stable channel (VERIFIED, `index.json`, `files[].last_modified`):
  - 154.0.23 (Chromium 154.0.8037.17): 2026-09-22.
  - 154.0.34 (Chromium 154.0.8037.98): 2026-10-05.
  - 156.0.7 (Chromium 156.0.8078.17): 2026-10-22. Present on linux64, linuxarm64, macosarm64, macosx64, windows64.
- cef-rs lag: 154.0.23 was published to crates.io on 2026-09-23 (one day after CEF). 154.0.34 was published on 2026-10-07 (two days after CEF). cef-rs follows CEF point releases within about 2 days (INFERRED from two data points).
- CEF 156 is not in cef-rs yet. The sibling note (`01-cef-on-helium.md`) records CEF master moving its Chromium pin to 156.0.8078.0 on 2026-10-07 (VERIFIED by sibling, `CHROMIUM_BUILD_COMPATIBILITY.txt`). So cef-rs is one milestone behind CEF master.
- Status conflict, UNKNOWN: the CEF build index labels 156.0.7 as `stable`. The wiki branch table (via sibling note) labels milestone 156 "Beta". I did not resolve this. Recheck when Chromium 156 stable ships.
- CEF `CHANGELOG`: `cef/CHANGELOG.md` has an `[Unreleased]` section with one fix and then the 152.2.0 entry (VERIFIED, `cef/CHANGELOG.md` L7–L12). Binding bumps are not logged there. Track changes via commits.

### 1.4 Stated status

- README: "Use CEF in Rust." Plus a supported-targets table of Linux, macOS, and Windows for x86_64 and ARM64 (VERIFIED, `README.md` L1–L9). No stability or API-stability statement.
- docs.rs: no stability statement and no Chromium version stated in the crate page. Latest version 154.5.0+154.0.34, release date 2026-10-07 (VERIFIED, `https://docs.rs/crate/cef/latest`, via WebFetch summary; an earlier WebFetch missed the version list, so recheck the page directly).
- Issue #456 (macOS arm64 SIGSEGV in a windowed browser on Google sign-in, filed 2026-08-19) was closed as COMPLETED on 2026-09-06. The one comment says it "looks like it's probably deep in the CEF library itself, or possibly in the upstream Chromium project" (VERIFIED, `gh issue view 456`). I did not find a fixing PR, and the cause is UNKNOWN.

### 1.5 Other Rust CEF crates on crates.io (VERIFIED, crates.io search and dependency endpoints)

- `cef-sys` 0.0.3, last updated 2015-12-11, repo `dylanede/cef-rs`. Dead.
- `bevy_cef_core`, `bevy_cef` 0.15.0 (2026-10-21). Normal deps include `cef`, `cef-dll-sys`. Bevy-specific.
- `tauri-runtime-cef` 3.0.0-alpha.6 (2026-10-14). Normal deps include `cef`, `cef-dll-sys`, `tauri-winit`. Tauri runtime on top of cef-rs. Alpha, and it couples us to Tauri.
- `welding` 0.15.0, `waterui-browser-cef` 0.5.1, `iced_webview_v2` 0.1.12. Each depends on `cef`.
- `kurogane` 0.0.6-beta.1 (created 2026-10-09, 24 downloads). Normal deps are only `objc2*`, so it likely uses its own binding. UNKNOWN.
- `wef` 0.6.0 (longbridge). No `cef`-named normal dependency. Likely its own binding. UNKNOWN.
- `buffr` on GitHub (`kryptic-sh/buffr`, pushed 2026-10-20, "Vim-inspired browser. Native, GPU-accelerated. Rust + CEF."). Its `Cargo.toml` pins `cef = "152"` (VERIFIED, `gh api repos/kryptic-sh/buffr/contents/Cargo.toml`). This is prior art for a Rust browser on CEF.
- `oxyblink` (0.1.0, 33 downloads), `wew` (mycrl), and `rdesktop-cef` are too small or unclear to assess here.

---

## 2. Platform support

### 2.1 Declared and built (VERIFIED)

- Declared targets: README table, Linux/macOS/Windows × x86_64/ARM64 (`README.md` L1–L9).
- `download-cef` target map: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (`download-cef/src/lib.rs` L67–L69). macOS `aarch64-apple-darwin` and `x86_64-apple-darwin` (L72). Platform fields `macosarm64`, `macosx64`, `linux64`, `linuxarm64` (L157–L164).
- Generated bindings exist for all four targets: `cef/src/bindings/{x86_64,aarch64}_{apple_darwin,unknown_linux_gnu}.rs` plus Windows and armv7 (`cef/src/bindings/`).
- CEF builds for all four exist in the index (154.0.34 and 156.0.7 entries, see 1.3).

### 2.2 CI (VERIFIED, `.github/workflows/rust.yml` L16–L70 and L73–L112)

- Matrix: `ubuntu-latest` and `macos-latest`. Steps: export CEF with `export-cef-dir`, then `cargo build --verbose` and `cargo test --verbose`.
- A separate job on `ubuntu-latest` cross-compiles `cefsimple` for `x86_64-pc-windows-msvc` with `cargo-xwin` (L73–L112).
- Not in CI: running any browser, `bundle-cef-app`, Linux arm64, macOS x86_64 (the matrix is `macos-latest` only, and the runner architecture is not pinned in the workflow; GitHub's label is arm64 today, UNKNOWN from this source), and Wayland.
- Unit tests that touch CEF: `cef/src/lib.rs` L45–L70 defines `test_init_cef()`, which loads the macOS framework and calls `initialize(...)`. It is `#[cfg(test)]`. I did not confirm which tests call it. Treat CI as a compile check plus minimal init, not as a runtime check (INFERRED).

### 2.3 macOS helper bundle and framework loading (VERIFIED)

- `cef/src/build_util/mac.rs` L228–L233 defines the helper variants: `Helper (GPU)`, `Helper (Renderer)`, `Helper (Plugin)`, `Helper (Alerts)`, and `Helper`. The loop at L83–L91 creates one `.app` per helper inside the main app's `Frameworks` directory. Helper bundles get `LSUIElement` (`ui_element`, L193).
- `cef/src/library_loader.rs` L9–L12 resolves the framework path. Helper processes look in `../../..`, the main process in `../Frameworks`. L26–L30 loads it with `dlopen`.
- `examples/cefsimple/src/main.rs` calls `shared::load_cef()` before `args` are parsed.
- `bundle-cef-app` is the bundler in this repo (`cef/src/bin/bundle-cef-app/`, with `mac.rs`, `linux.rs`, `win.rs`). Usage is in `README.md` ("Run the `cefsimple` Example"). The README shows a single `cefsimple.exe` run on Linux, which suggests no helper apps there (INFERRED; `linux.rs` not read).
- Not verified: whether the Linux bundle and the helper re-exec path actually work on arm64, and whether the helper list matches what the current CEF build expects. Experiment: bundle `cefsimple` on macOS arm64 and Linux arm64, then run it.

### 2.4 Linux display server (VERIFIED for cef-rs code; SECONDHAND for the Wayland claim)

- Bindings: `cef_window_handle_t` is `c_ulong` on Linux (an X11 window ID) and `*mut c_void` on macOS (`sys/src/bindings/x86_64_unknown_linux_gnu.rs` L17388; `x86_64_apple_darwin.rs` L17745).
- `cef_linux_window_properties_t.wayland_app_id` is bound (`cef/src/bindings/x86_64_unknown_linux_gnu.rs` ~L1062). `WM_CLASS` and `app_id` are set through it.
- cef-rs's cefsimple Linux window title code calls Xlib (`examples/cefsimple/src/shared/simple_handler/linux.rs` L3–L74; `x11-dl` dependency). That is X11-only.
- The `linux-x11` cargo feature in `cef/Cargo.toml` L47–L48 is an empty marker. It is only passed by `bundle-cef-app` as `-F linux-x11` (`cef/src/build_util/linux.rs` L67–L68). It gates no code.
- CEF's own GTK cefclient picks the GDK backend from the Ozone platform (`tests/cefclient/cefclient_gtk.cc` L146–L152). It sets `GDK_GL` for Wayland off-screen rendering (L110–L114). The `--ozone-platform` switch is in `tests/shared/common/client_switches.cc` L67. The comment near L97 says `--ozone-platform=x11` uses XWayland (VERIFIED, CEF master `7f8a5ec`).
- SECONDHAND (search snippet, not opened): the Level1Techs thread relays the maintainer as saying Views are how Ozone reaches Aura, and that Views are needed to run Ozone on Wayland. The Collabora readme says Views are required, and gives `--use-views --ozone-platform=wayland` as the launch line.
- Not verified: whether cef-rs builds and runs under Wayland at all. No Wayland CI, no Wayland issue or example in the repo. Experiment: run `cefsimple --use-views --ozone-platform=wayland` on Hyprland and record the result.

---

## 3. Windowing model

### 3.1 What cef-rs exposes (VERIFIED)

- Views C API, auto-wrapped: `window_create_top_level`, `browser_view_create`, `panel_create`, `browser_view_get_for_browser`. These are present as free functions in `x86_64_unknown_linux_gnu.rs`.
- Views methods on `View` (`cef/src/bindings/x86_64_unknown_linux_gnu.rs`): `set_to_box_layout(settings)` at L41689, `set_to_fill_layout` at L41687, `add_child_view` at L41695, `add_child_view_at` at L41697. There is no free function `box_layout_create`. Box layout is set on the parent view.
- Native parent: `WindowInfo::set_as_child(parent, bounds)` (`cef/src/window_info.rs` L6–L7). On Linux it sets `parent_window`, an X11 window ID. On macOS it sets `parent_view` (L12–L14).
- Windowless (OSR): `WindowInfo::set_as_windowless` (`window_info.rs` L49).

### 3.2 What cef-rs's own example does (VERIFIED)

- `examples/cefsimple/src/shared/simple_app.rs`: "Views is enabled by default (add `--use-native` to disable)" (around L150). Views path: `browser_view_create` (L159), `window_create_top_level` (L188), then `on_window_created` adds the BrowserView to the Window with `window.add_child_view` and shows it (L25–L36).
- The example uses one BrowserView. It does not show two.
- `examples/osr` uses winit and wgpu with `browser_host_create_browser_sync` (`examples/osr/src/main.rs` L299).

### 3.3 Two views in one window: CEF's own precedent (VERIFIED, `tests/cefclient/browser/views_window.cc`, CEF master)

- L152–L158: "Only create an overlay browser for a primary window." Gated by `--show-overlay-browser`.
- L160–L170: the main BrowserView's runtime style is CEF_RUNTIME_STYLE_ALLOY or CHROME depending on `use_alloy_style_`. A `CHECK_EQ` asserts the BrowserView matches. The window style is asserted separately (L163, L177).
- L1055–L1066: the overlay is a second `ViewsOverlayBrowser`, initialised with `DefaultClientHandler(/*use_alloy_style=*/true)`. So cefclient runs a CHROME-style (or ALLOY) main view and an ALLOY overlay view in one CefWindow.
- Implication: two BrowserViews in one window are a supported, exercised pattern. Mixing runtime styles in one window is also exercised, at least in CEF's test app.
- Caveat: the Chrome toolbar is "Only supported when using Chrome style" (`include/views/cef_browser_view.h` L85–L89). Chrome-style browser features (toolbars, bubbles, some accelerator behaviour) depend on the style. Test sidebar behaviour before choosing.

### 3.4 Runtime style

- cef-rs binds `RuntimeStyle { DEFAULT, CHROME, ALLOY }` (`cef/src/bindings/x86_64_unknown_linux_gnu.rs` L45718–L45744). It is a per-BrowserView setting through `GetBrowserRuntimeStyle` (`include/views/cef_browser_view_delegate.h` L169–L174) and a query through `GetRuntimeStyle` (`cef_browser_view.h` L112–L116).
- `cef_types.h` on master has no `chrome_runtime` setting field. The runtime choice is per browser view and per window, not a global setting (VERIFIED by absence in `include/internal/cef_types.h`).
- I did not locate the `cef_runtime_style_t` enum definition in the headers I fetched. The cef-rs bindings are the usable source.

### 3.5 Which approach CEF recommends

- cef-rs's cefsimple defaults to Views (`--use-native` opts out). CEF's shared test switches include `use-views`, `use-native`, and `use-chrome-style-window` (`tests/shared/common/client_switches.cc` L38–L39 and L64). Which default CEF's own cefclient uses on each platform was not checked.
- SECONDHAND: the Collabora readme and Level1Techs thread say Views are required for Wayland.
- UNKNOWN: a single maintainer statement on Views vs native for a full browser. The General Usage wiki page is not reachable from here (the GitHub wiki clone failed). The CEF forum and the wiki are the next sources to check.

### 3.6 Recommendation

Use CEF Views for both platforms:

- One `CefWindow` (`window_create_top_level`), one root `Panel` with a horizontal `BoxLayout` (`set_to_box_layout`).
- A content `BrowserView` in a flex child.
- A sidebar `BrowserView` (fixed width) as a second child.
- Use `ALLOY` for the sidebar, following the cefclient overlay precedent, unless the prototype shows Chrome style is needed.

Rejected: native NSView child browsers on macOS (possible through `set_as_child`, but it means managing layout and focus ourselves, outside Views). Native X11 child browsers on Linux (the parent handle is an X11 XID, so Wayland is out). Overlay-only (no precedent for a permanent sidebar).

---

## 4. IPC

### 4.1 Primitives CEF gives and cef-rs binds (VERIFIED)

- Browser to renderer: `Frame::send_process_message(ProcessId::RENDERER, Some(&mut message))`. Used in `cef/src/wrapper/message_router.rs` L569 and L597. `process_message_create` is a bound free function.
- Renderer to browser: `on_process_message_received`. Bound in the `Client`/`RenderProcessHandler` wrapper traits (6 declarations in `x86_64_unknown_linux_gnu.rs`). Used in `message_router.rs` L183–L190 (browser side) and L227–L234 (renderer side).
- V8: `v8_value_create_function` is bound. cef-rs wraps `V8Handler` with `wrap_v8_handler!` (`message_router.rs` L1625).
- CefMessageRouter port: `MessageRouterConfig` (`message_router.rs` L17–L39) with `js_query_function` default `"cefQuery"` (L38) and cancel `"cefQueryCancel"`. Traits `MessageRouterBrowserSide` (L129) and `MessageRouterRendererSide` (L196), with callbacks at L163 and L207.
- Custom scheme: `register_scheme_handler_factory` and `clear_scheme_handler_factories` are bound (`x86_64_unknown_linux_gnu.rs`). `ResourceManager::new` (`cef/src/wrapper/resource_manager.rs` L722) and `add_content_provider` (L738) serve content from providers. `StreamResourceHandler` and `zip_archive.rs` are in the same folder.
- JS execution: `Frame::execute_java_script` (2 declarations bound).

### 4.2 Gaps in the wrapper (VERIFIED by grep)

- No example in this repo constructs `MessageRouterBrowserSide` or calls its `add_handler`. Grep for `MessageRouterBrowserSide::new` and `add_handler` in `examples/` and `cef/src/` returns nothing outside the trait definitions.
- The renderer hook in `examples/tests_shared/src/renderer/client_app_renderer.rs` L140–L153 and L234–L246 forwards `on_context_created` and `on_process_message_received` to a delegate list. I did not confirm that a delegate instantiates the router.
- So `MessageRouter*` is ported but unexercised in this repo (INFERRED from absence; the port itself is VERIFIED).

### 4.3 Answer to "without a custom V8 binding?"

Yes, two ways, neither requires writing a V8 binding.

1. Pull via custom scheme. The web UI calls `fetch("app://core/...")` (POST for requests). The browser-side scheme handler (Rust) handles it and returns a response. This is request/response only and needs no renderer code. It works in both views.
2. Push via `execute_java_script` from Rust into a frame, or `send_process_message` to the renderer.

For cefQuery-style calls, the router needs both sides registered. That is the wrapper path cef-rs has not exercised here.

Between two views: no direct CEF channel. Route through the Rust core: view A sends to Rust, Rust sends to view B with `execute_java_script` or a process message (INFERRED design, not tested).

Custom schemes must be registered in every process. CEF docs (`include/cef_app.h` L222–L227): "called on the main thread for each process and the registered schemes should be the same across all processes." cef-rs does this in both `client_app_browser.rs` L179 and `client_app_renderer.rs` L270 (VERIFIED). The sidebar and content views must agree on the scheme set.

---

## 5. Custom features

Exposure is from the generated wrapper traits (`cef/src/bindings/x86_64_unknown_linux_gnu.rs`; generated by `update-bindings/src/parse_tree.rs` L2670). Each `wrap_*!` macro implements one CEF handler. Callback names are verified there. Behaviour at runtime is not verified.

| Feature | CEF callbacks / methods (bound) | cef-rs status |
|---|---|---|
| Custom scheme (`app://ui`) | `on_register_custom_schemes`, `register_scheme_handler_factory`, `wrap_scheme_handler_factory!`, `ResourceManager` | Exposed. `on_register_custom_schemes` is implemented in `tests_shared` (both processes). Scheme-handler runtime use not checked |
| Request interception | `on_before_resource_load`, `wrap_resource_request_handler!`, `wrap_request_handler!`, `wrap_resource_handler!` | Exposed |
| Download | `on_before_download`, `on_download_updated`, `wrap_download_handler!` | Exposed, not exercised |
| Permission prompts | `on_show_permission_prompt`, `on_request_media_access_permission`, `wrap_permission_handler!` | Exposed, not exercised |
| Context menu | `on_before_context_menu`, `on_context_menu_command`, `wrap_context_menu_handler!` | Exposed |
| Find in page | `BrowserHost::find` (2 decls), `on_find_result`, `wrap_find_handler!` | Exposed. Issue #238 (⌘F on macOS) closed |
| Printing | `BrowserHost::print`, `print_to_pdf` (2 decls each), `wrap_print_handler!` | Exposed |
| PiP / popups | `on_before_popup`, `on_popup_browser_view_created` (3 decls), `wrap_browser_view_delegate!` | Exposed. Document PiP: UNKNOWN |
| DevTools | `BrowserHost::show_dev_tools`, `close_dev_tools` (2 decls each) | Exposed |
| JS / file dialogs | `on_jsdialog`, `on_file_dialog` (`wrap_jsdialog_handler!`, `wrap_dialog_handler!`) | Exposed |

Caveats:

- The generated macros are hard to use. Issue #297 (open) says they cannot be formatted by `rustfmt`, have no IntelliSense, and need the exact class order. That is a DX cost in every handler we write.
- Handler coverage is not runtime coverage. The example suite exercises only a few of these (`examples/tests_shared` covers scheme, client, life span, and display handlers). Each feature needs a test in our prototype.

---

## 6. Alternatives

- **cef-rs (`cef` / `cef-dll-sys`)**: Use. Covers the full C API, and CEF's `libcef_dll_wrapper` is built through CMake from `sys/build.rs`. Risk is churn and single-team cadence (Section 7).
- **Raw bindgen against the CEF C API**: `cef-dll-sys` already does this (`update-bindings` uses bindgen 0.73 per `Cargo.toml` workspace deps). Rolling our own gives no advantage over cef-rs unless cef-rs stops tracking CEF.
- **`cef-sys` 0.0.3 (dylanede)**: Dead since 2015. Do not use.
- **Thin C++ shim over CEF's C++ API (`libcef_dll_wrapper`)**: Fallback only. Adds a C++ build and an FFI surface. Use it only if cef-rs's Views or handler wrapping proves unworkable, and do not build it up front.
- **Downstream Rust crates (`tauri-runtime-cef`, `bevy_cef`, `welding`, `waterui-browser-cef`, `iced_webview_v2`)**: Each depends on cef-rs. They prove the binding is in use, but none is a host we would adopt. `tauri-runtime-cef` is alpha and ties us to Tauri.
- **`buffr`**: Rust + CEF browser (pins `cef = "152"`). Useful reference for a Rust browser shell on CEF. Read its `crates/buffr-cef` source before the prototype (not opened in this note).

---

## 7. Prototype scope and hard parts

Prototype scope to test the risky parts:

1. One `CefWindow`, one root `Panel` with horizontal `BoxLayout`, one content `BrowserView`, one sidebar `BrowserView` (ALLOY). Confirm both render and resize.
2. Message passing: sidebar to Rust core, Rust core to content view, via the custom scheme (pull) and `execute_java_script` (push). Then try the router only if the scheme path is too limited.
3. macOS arm64: `bundle-cef-app`, run the bundle, confirm helper processes start, and CefRunMessageLoop works with our Rust host.
4. Ubuntu x86_64 under Wayland (Hyprland): `--use-views --ozone-platform=wayland`. Record whether it starts and whether the two views render.
5. Repeat the Views window on Chrome style and compare the sidebar.

Hard parts I expect:

- macOS NSApplication. cef-rs's cefsimple defines an NSApplication subclass conforming to `CefAppProtocol` (`examples/cefsimple/src/mac/mod.rs` L17–L120; the conformance comment is at L105). Our Rust host has to provide that class, or use a message pump CEF accepts. `multi_threaded_message_loop` is documented as "only supported on Windows and Linux" (`include/internal/cef_types.h` L249–L250). So on macOS we need `CefRunMessageLoop` or an external pump, and both interact with the event loop.
- Wayland. Views is the only path we have evidence for, and cef-rs has no Wayland test. Expect Ozone and Aura issues. CEF's cefclient GTK path has Wayland branches (`cefclient_gtk.cc` L146–L152), so the Linux side may need a GTK or Ozone-aware setup.
- Mixed runtime style in one window. Precedent exists in cefclient (ALLOY overlay), but nothing tests our sidebar's behaviour (focus, accelerators, drag regions, context menus).
- Handler DX. Each handler is a `wrap_*!` macro. Budget time for that (Section 5).
- CEF version churn. cef-rs is one major behind CEF master (154 vs 156). Expect a bump during the prototype, and expect `wrap_*` macro changes with it.

---

## Could not verify

- Whether cef-rs runs on Wayland at all. No test, no example, no issue.
- Whether two BrowserViews in cef-rs render and route input correctly on macOS or Linux. CEF's cefclient does it in C++, but cef-rs has no example.
- Whether Alloy-style sidebars keep the features we need (permission prompts, print, PiP, context menu, accelerators).
- Whether the helper bundle works on Linux arm64 or macOS x86_64.
- Whether CEF 156 is stable. The index says yes. The sibling note reads the wiki as Beta.
- Whether `macos-latest` is arm64. Not stated in the workflow.
- Whether `kurogane` and `wef` use their own bindings. I did not open their source.
- docs.rs stability statement. None found in the summary I got; recheck the page directly.
- Secondhand Wayland claims (Level1Techs, Collabora). Not opened.

Experiments that settle these: (a) bundle and run cefsimple on macOS arm64 and Linux arm64; (b) cefsimple `--use-views --ozone-platform=wayland` under Hyprland; (c) a two-BrowserView Views prototype with ALLOY and CHROME sidebars; (d) an NSApplication subclass with `CefRunMessageLoop` on macOS; (e) a `fetch("app://…")` round-trip plus `execute_java_script` push between two views.

## Verdict

**Go with caveats** for cef-rs as the CEF binding layer. It is maintained, covers the full C API, is the base for the other Rust CEF projects on crates.io, and its own examples run Views. Do not commit to it for production until the prototype passes on macOS arm64 and Linux under Wayland.

**Recommended windowing approach:** CEF Views. One `CefWindow`, a root `Panel` with horizontal `BoxLayout`, a content `BrowserView`, and a sidebar `BrowserView` (ALLOY by default). Native NSView or X11 child parenting is rejected (Section 3.6).

**Top 3 risks:**

1. **Untested runtime paths.** CI builds and runs unit tests only. No browser launch, no bundle, no Wayland, no Linux arm64, no macOS x86_64 runtime. The first milestone must show each platform running a window, not just compiling.
2. **macOS integration with a Rust-owned event loop.** Needs an NSApplication subclass conforming to `CefAppProtocol` (cefsimple's `define_class`, `examples/cefsimple/src/mac/mod.rs`) and a supported message pump. The multi-threaded loop is not supported on macOS (`include/internal/cef_types.h` L249–L250). Issue #456 (macOS arm64 crash in a windowed browser) was closed as COMPLETED with no stated fix.
3. **API and version churn from a small lead team.** Each CEF point release triggers binding updates every few days, and cef-rs is one milestone behind CEF master (154 vs 156). Handler macros are hard to use (#297). The CHANGELOG does not log binding changes, so we need our own pin and bump process.
