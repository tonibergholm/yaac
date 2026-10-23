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

## 2026-10-23: spike results (E1, E2, prototype)

Source: `docs/spikes/`. The prototype code is on the local branch `spike/two-views` and is not merged.

- **Patch order reversed: Helium first, then CEF** (E1, observed). CEF alone on Helium's Chromium base: 117 of 117 patches clean. CEF then Helium: 15 Helium patches rejected (2 of those are missing-input ublock patches present in either order). Helium then CEF: 5 CEF patches rejected in archive mode (`--chromium-archive`), 10 in checkout mode. The overlap numbers in note 01 reproduce exactly on CEF master (96 shared paths, 32 overlapping files). This supersedes note 01's "CEF first" recommendation. Not tested: domain substitution, pruning, GN.
- **`cef-rs` Views skeleton: go on macOS arm64** (prototype, observed). `cef =154.5.0`, one window, ALLOY sidebar and content views, both IPC directions, clean exit. Not tested: mixed runtime styles, Cmd-Q keystroke, Linux.
- **User-namespace sandbox works on Omarchy** (E2, observed). `cefclient` 156.0.7 starts without `--no-sandbox`. `chrome-sandbox` is not setuid. Our package does not need a setuid sandbox on this kernel.
- **Pass `--ozone-platform=wayland` explicitly** (E2, observed). A Wayland session alone does not select it.
- **Disable external extension providers.** On Omarchy a system file under `/usr/share/chromium/extensions/` with no package owner made CEF install an extension. `--disable-extensions` stopped it.

Open:

- Stock CEF opened connections to Google-owned hosts with local-only pages, despite the usual `--disable-*` switches. Cause unknown. Find it with a net log before any "no network" claim.
- Whether `cefclient` paints on Hyprland. The E2 captures show only the desktop and an "Enter Pa..." box. Omarchy's idle monitor (screensaver at 150 s, lock at 300 s) cycled during the runs, so this may be a test artifact, not a CEF fault. Rerun with idle disabled and the user present.
- Hyprland titlebar behavior, GPU page, popups, IME, screen sharing (need the user at the machine).
- Mixed Chrome-style and Alloy views in one window.
- CEF 156 stable vs beta, and MV2 component loading under CEF's Chrome runtime.

## 2026-10-23: no built-in AI, but a first-class agent interface

- No built-in AI features. This still holds.
- The browser must be usable by external agents through a CLI and an MCP server, so agents do not need screenshot-and-click automation. The owner needs this daily at work, so it is in scope for the first usable build, not a later extra.
- Token efficiency is a design requirement. Page state goes to agents as compact accessibility-tree snapshots with stable element refs, not DOM dumps or screenshots by default. Responses are size-bounded.
- Safe by default: off until enabled, local-only transport (Unix socket or named pipe with filesystem permissions, no TCP port), explicit per-Space grants, a visible agent indicator, and an audit log. Details follow the research in `docs/research/05-agent-interface.md` and land in `docs/architecture.md`.
- One typed command API in the Rust core serves the web UI, the CLI and the MCP server. Not a separate CDP proxy. Not yet decided; the architecture spec will confirm.
- Reference: Aside's documented CLI, MCP and REPL. Observed behavior only; no Aside code or assets.

## 2026-10-23: agent interface, owner answers

Answers to the open questions in `docs/research/05-agent-interface.md`.

- `evaluate` ships in v1, in the privileged scope, with confirmation.
- Agents act only in Spaces the user created or marked for agents. Never the default profile.
- Confirmation is required only for sensitive actions: payment and credential fields, account settings, and origins on a user-managed list. The exact definition goes in `docs/architecture.md`.
- Cookie access is off by default. A per-Space toggle in settings turns it on. When on, it is a separate privileged scope.
- The MCP tool schema and the CLI contract are public and versioned from the first release.
- Windows transport (a named pipe with an owner-only access list) is deferred. Windows is not a v1 target.

