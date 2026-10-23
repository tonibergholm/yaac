# Decision log

## 2026-10-23: initial direction

- Engine is Chromium through CEF. We do not write an engine.
- Rust owns everything except the engine and the UI markup.
- Browser chrome is web UI rendered in CEF, talking to the Rust core over IPC.
- Platform code lives behind a narrow trait boundary. macOS arm64 first, Linux second.
- No AI features in v1.
- Clean room: observe Arc's behavior, never copy its code or assets.
- GPL-3.0 for the combined app, since Helium's patches require it. Check each reused file's header before copying.
- Repository is public from the start: `tonibergholm/yaac`.

Open until the feasibility spike reports: CEF on a Helium-patched Chromium, `cef-rs` maturity, extension and MV2 support.
