# Decision log

## 2026-10-23: initial direction

- Engine is Chromium through CEF. We do not write an engine.
- Rust owns everything except the engine and the UI markup.
- Browser chrome is web UI rendered in CEF, talking to the Rust core over IPC.
- Platform code lives behind a narrow trait boundary. macOS arm64 first. The first Linux target is Omarchy (Arch, Hyprland, Wayland-first); other distros come later.
- No AI features in v1.
- Clean room: observe Arc's behavior, never copy its code or assets.
- GPL-3.0 for the combined app, since Helium's patches require it. Check each reused file's header before copying.
- Repository is public from the start: `tonibergholm/yaac`.

## 2026-10-23: feasibility research (task 1, before any prototype)

Source notes: `docs/research/01` to `04`. Nothing was built or run; all findings are from source reading.

- CEF on Helium: feasible with caveats. Apply CEF's patches first, then Helium's. Known conflicts: RLZ (CEF forces `enable_rlz=true`, Helium disables it) and 32 files with overlapping hunks.
- `cef-rs` as the Rust binding: go with caveats. Use CEF Views (one window, content view plus sidebar view). Not native view parenting.
- Extensions: bundle uBlock Origin MV2 as a built-in component, with a patch that keeps MV2 enabled. CEF has no public extension API since 2024.
- Linux: conditional go for CEF Views on Ozone/Wayland. Embedding CEF into a foreign Wayland surface is not available.
- Open until the prototype settles them: CEF 156 stable vs beta, Hyprland titlebar behavior, `chrome-sandbox` setuid handling, MV2 component loading under CEF's Chrome runtime.
