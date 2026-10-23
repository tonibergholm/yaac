# Two-view Views spike: cef-rs on macOS arm64

Date: 2026-10-23. Branch `spike/two-views`, commit `d7d026c`. Code in `prototype/`.

Labels: VERIFIED = observed in this run or read in source. INFERRED = reasoning from verified facts.

## Versions

- `cef` crate pinned to `=154.5.0` (version `154.5.0+154.0.34`). VERIFIED from the cef-dll-sys build log and `cef/Cargo.toml`.
- CEF 154.0.34, Chromium 154.0.8037.98, macOS arm64, minimal archive (126 MB download). VERIFIED from the cache directory name and the `CEF_API_VERSION=15400` build flag.
- CEF 156 was not used.
- The download uses download-cef, which checks SHA1 against the index (VERIFIED from download-cef source, lines 256 to 323). I did not compare the archive hash by hand.

## Build and run

Everything writes only into the worktree (ignored `target/` and `bundle/`), `~/Developer/yaac-spike-cache/`, and `~/Developer/yaac-spike-results/`.

```sh
export CARGO_HOME=~/Developer/yaac-spike-cache/cargo-home
export CEF_PATH=~/Developer/yaac-spike-cache/cef
export PATH=~/Developer/yaac-spike-cache/tools/py/bin:~/Developer/yaac-spike-cache/tools/py/cmake/data/bin:$PATH
cd prototype
cargo build
cargo install cef --version "=154.5.0" --locked --debug --bin bundle-cef-app \
  --root ~/Developer/yaac-spike-cache/bundler --target-dir ~/Developer/yaac-spike-cache/bundler-target
~/Developer/yaac-spike-cache/bundler/bin/bundle-cef-app yaac-spike -o bundle -i dev.yaac.spike-two-views -d yaac-spike
codesign --force --deep -s - bundle/yaac-spike.app
bundle/yaac-spike.app/Contents/MacOS/yaac-spike --selftest > proto-selftest.log
bundle/yaac-spike.app/Contents/MacOS/yaac-spike --selftest --keep-open   # window stays up for screencapture
```

Tooling notes:
- cmake 4.4.4 and ninja 1.13.2 were installed with `pip install --target` into the cache dir. No Homebrew or other system packages were installed. VERIFIED.
- The bundler and the cef-rs build need `CEF_PATH` and cmake on PATH. The bundler runs `cargo build` for both bins from the current directory.

## What worked (VERIFIED)

- One CefWindow with a root Panel and a horizontal BoxLayout. Sidebar is a 240 px BrowserView on the left. Content is a flex BrowserView filling the rest. Both are ALLOY. Logged layout: window 1000x640, sidebar 240x608 at 0,0, content 760x608 at 240,0.
- Pages are served from `app://ui/...` by a custom scheme handler, registered in the browser and helper processes through `on_register_custom_schemes`. The sidebar's `fetch()` calls reached the scheme handler, so the scheme works in the renderer.
- Sidebar to Rust: a click on "Page B" runs `fetch('/api/navigate?page=b')`. Rust checks the page against an allowlist and calls `load_url` on the content frame.
- Rust to sidebar: content title and URL changes are pushed with `execute_java_script('window.yaacPush(<json>)')`. The sidebar renders them and acks with `fetch('/api/ack?...')`.
- Trust boundary: the content view calling `/api/navigate` is refused with 403, and Rust logs the refusal. The selftest requires this.
- Selftest: passes in about 0.3 to 0.9 s and exits 0. The log is `proto-selftest.log`.
- Clean exit: selftest close, plain launch closed by Apple Event quit (goes through `terminate:`), and keep-open quit all exit 0 with "both browsers closed" logged first. No DiagnosticReports entry for this process.
- Screenshot: `proto-two-views.png` (2224x1504, taken with `screencapture -x -l <windowNumber>`) shows both views: sidebar with the pushed title "Page B", and the content view showing Page B.
- The helper bundles (Helper, GPU, Renderer, Plugin, Alerts) are present and run. The main process uses `sandbox` through the helper; the sandboxed renderer rendered the pages. VERIFIED that pages render. The sandbox setting itself was not checked independently.

## What did not work or is unresolved

1. Screen capture. Two early attempts failed with `could not create image from window` and `could not create image from display` (exit 1). The same command later succeeded (exit 0) without any change I know of. The cause is UNKNOWN. Check the Screen Recording permission for the host app if capture fails again.
2. Network egress (the most important open item). `lsof` on the running browser process showed ESTABLISHED TCP connections to Google-owned hosts (`lr-in-x54.1e100.net`, `lq-in-x8b.1e100.net`, and `2001:4860:482d:7700::`). This happened with local-only pages. It still happened after adding `disable-background-networking`, `disable-component-update`, `disable-sync`, `disable-domain-reliability`, `safebrowsing-disable-auto-update`, `disable-client-side-phishing-detection`, `metrics-recording-only`, `no-pings`, and a `disable-features` list. The service is unknown. The sample was taken at one point in time, about 3 s into a run. So the "no network" constraint is not met. Next step: capture a net log (`--log-net-log`) and identify the host, or check it with nettop.
3. Push ordering. A content navigation emits an address change before the title change. The first push therefore pairs the new URL with the old title ("Start page" with `b.html`, seq 2). The ack reports displayed=true for that intermediate state. Harmless for the spike, but a skeleton needs coalescing.
4. The ack check is weak. The sidebar sets `textContent` and then compares it with the same string, so the check can never fail. The screenshot is the real visual evidence. A real check needs the page to send back what it read from the DOM.
5. Click is synthetic (`element.click()` via injected JS), not native input. Cmd-Q as a keystroke was not tested. Sending keystrokes needs Accessibility permission, which I did not have. The Apple Event quit path was tested, and it runs the same `terminate:` override that Cmd-Q would hit.
6. Popups: `on_before_popup` returns 1 and logs. Not exercised by the test.
7. Runtime style mixing: not tested. Both views and the window are ALLOY. I did not read `cef_runtime_style_t` in the headers. Research note 02 §3.3 (cefclient ALLOY overlay in a CHROME window) is the only evidence, and it is secondhand for this spike.
8. The macOS ad-hoc signature from the bundler was invalid after bundling (`codesign --verify` reported missing resources). Fixed with `codesign --force --deep -s -`. VERIFIED.
9. No cef-rs issue numbers were hit. cef-rs issue #456 (windowed browser SIGSEGV on macOS arm64) did not reproduce in Views mode. This is one run, not a stress test.

## macOS event loop

- The cefsimple `SimpleApplication` pattern (NSApplication subclass implementing `CefAppProtocol`, set up before `initialize`) worked as copied. `run_message_loop()` with no external pump was used. VERIFIED.
- Changes from cefsimple: `terminate:` closes the window through the normal close path (`can_close` calls `try_close_browser` on both browsers without short-circuiting). A Quit menu item is built in code, because there is no MainMenu nib. Activation policy is set with `msg_send` to avoid an extra objc2 feature.
- Difficulty: low. I hit no event-loop problems. The main risk is the multi-browser close sequence, which worked in every run.

## Go or no-go

**Go** for building the real skeleton on cef-rs with CEF Views. The windowing model, the custom scheme, both IPC directions, the bundle with helpers, and clean quit all work on macOS arm64. VERIFIED.

Conditions before the skeleton relies on it:
1. Find the source of the Google-hosted connections and remove them, or accept them explicitly. This blocks any "no network" claim.
2. Test mixed runtime styles (CHROME sidebar or content) in one window.
3. Test Linux and Wayland (`--ozone-platform=wayland`). Not started, per scope.
4. Replace the weak DOM ack with a real read-back check, and coalesce pushes.

## Review

A code-reviewer pass ran on the first build. Its blocking finding (plain-launch exit code was 1 on close) was fixed and re-verified with a plain launch and Apple Event quit (exit 0). The reviewer's other points were applied: popup deny, `terminate:` sender typed as optional, and the ack log label. Layout logging moved to the selftest decision point. A renewed review of the final diff has not been run.

## Results directory

Files I created here: `proto-selftest.log`, `proto-selftest.stderr.log`, `plain-quit-stdout.log`, `plain-quit-stderr.log`, `hold-stdout.log`, `hold-stderr.log`, `proto-two-views.png`, `proto-report.md`. The file `e2-r1-views-wayland-sandbox.png` was not created by this run and was left alone.
