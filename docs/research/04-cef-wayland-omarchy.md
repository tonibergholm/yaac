# 04. Can a CEF browser target Wayland first on Omarchy?

Accessed: 2026-10-23. Read-only research. No project code written, no builds run. Sources were read through GitHub, googlesource, the AUR, and the Omarchy repo. Nothing was executed on a Hyprland machine.

Pinned sources:

- CEF: `chromiumembedded/cef` master, commit `7f8a5ec28e932aa62b583c8248e3df2e4dfe4807`. Permalinks below use this commit.
- cef-rs: `tauri-apps/cef-rs` dev, commit `fe2971b855c650b2a1b403b7c3f3521100e8b8fc`.
- Omarchy: `basecamp/omarchy` main, commit `967bbdd7346e6e2ff1a6af33c5d762ea784832cc`. Version file reads `4.0.0.alpha`.
- Hyprland: `hyprwm/Hyprland` main as of 2026-10-23. `src/protocols/XDGDecoration.cpp` last changed in commit `59177602ffd75d81e8995e0456057e2d086a01c8` (2026-09-02).
- Chromium: no commit pin. Files read from `chromium.googlesource.com` main and the `chromium/chromium` GitHub mirror on 2026-10-23.
- CEF build index `https://cef-builds.spotifycdn.com/index.json`, downloaded 2026-10-23.

Labels: **VERIFIED** means read in a primary source. **INFERRED** means my reasoning from verified facts. **SECONDHAND** means a search snippet or forum summary I did not confirm in the source. **UNKNOWN** means not determined, with the experiment that would settle it.

Previous report `docs/research/02-cef-rs.md` is still accurate on the points checked here. This report adds the Wayland-specific evidence.

---

## Summary

- CEF supports Ozone/Wayland only through the Views framework. Windowed browsers parented to a foreign native window are not supported on Wayland. The embedding feature is an open PR awaiting an API decision. (VERIFIED)
- Chromium picks Wayland automatically when `XDG_SESSION_TYPE=wayland` and no `--ozone-platform` switch is set. CEF runs that code. (VERIFIED by source reading)
- Omarchy's Chromium flags (`--ozone-platform=wayland`, `--ozone-platform-hint=wayland`) are the same pattern. The hint flag does not appear in Chromium's code. (VERIFIED for Omarchy file, INFERRED for the hint being a no-op)
- Server-side decoration is forced on Hyprland. Frameless CEF windows therefore depend on the compositor or a Hyprland plugin for their title bar. Not tested. (VERIFIED for Hyprland code, UNKNOWN for the visible result)
- Runtime behavior on Hyprland, NVIDIA, IME, drag and drop, screen sharing, and multi-view input was not verified. Every such claim below is UNKNOWN or INFERRED, with an experiment attached.

---

## Omarchy stack (VERIFIED from `basecamp/omarchy` at `967bbdd`)

- Compositor and session: Hyprland, configured in Lua under `default/hypr/`. Start-up through `uwsm` and login through `sddm` (`install/omarchy-base.packages`).
- Session environment: `XDG_SESSION_TYPE=wayland`, `OZONE_PLATFORM=wayland`, `ELECTRON_OZONE_PLATFORM_HINT=wayland`, `GDK_BACKEND=wayland,x11,*`, `QT_QPA_PLATFORM=wayland;xcb` (`default/hypr/envs.lua` L11-L18).
- Portals: `xdg-desktop-portal-hyprland` and `xdg-desktop-portal-gtk`. Routing is `default=hyprland;gtk`, and `org.freedesktop.impl.portal.Secret` goes to `gnome-keyring` (`config/xdg-desktop-portal/hyprland-portals.conf`).
- Keyring: `gnome-keyring` and `libsecret` are base packages. The default Chromium flags use `--password-store=gnome-libsecret` (`config/chromium-flags.conf` L3).
- Notifications: a Quickshell notifications plugin (`shell/plugins/notifications/`).
- Input methods: fcitx5 with mozc, hangul, and chewing addons (`install/omarchy-base.packages`).
- Audio: wireplumber is a base package. No `pipewire` line appeared in `omarchy-base.packages`. Other package lists were not checked (UNKNOWN for PipeWire's source).
- Browser: system `chromium` is a base package, with flags in `config/chromium-flags.conf`.
- Packaging: pacman plus AUR through `yay` (base packages).
- Titlebars: `omarchy-hyprland-titlebars` plugin (`default/hypr/workspace-modes.lua` L26 loads it).

## 1. CEF on Wayland: state of the Ozone backend

### 1.0 Branch scope

- Every CEF source read here is on `master` at `7f8a5ec`, not on a release branch. The CEF repo's numeric branches are 8037 (Chromium 154) and 8078 (Chromium 156, labelled Beta in `docs/branches_and_building.md` L85). `git ls-remote --heads` lists no 155 branch, and the build index has no 155 builds. CEF tracks even-numbered Chromium milestones (`docs/branches_and_building.md`, VERIFIED).
- The index labels 156.0.7 as both `stable` and `beta` (VERIFIED from `index.json`). The branch table says Beta. I did not resolve the conflict.
- Not done: diffing the Linux Views and Ozone files between master and branch 8078. Those diffs would show whether the 156 branch differs from master on the files cited here. UNKNOWN until run (`git fetch --depth 1 origin 8078`).

### 1.1 Views is the supported path (VERIFIED)

- Maintainer statement, 2022-01-31, on issue #2804: "Ozone builds can currently run with different platform backends (Wayland, X11, etc.). Usage of the Views framework is required, and the cefclient sample application is not supported. See issue #2296." Source: `https://github.com/chromiumembedded/cef/issues/2804` (comment list, author `magreenblatt`).
- `tests/cefclient/cefclient_gtk.cc` L91-L98 refuses windowed (non-Views) browsers on Wayland: "Native parent windows are not supported with Wayland. Run with --ozone-platform=x11 (uses XWayland) or without --use-native." Source: `https://github.com/chromiumembedded/cef/blob/7f8a5ec28e932aa62b583c8248e3df2e4dfe4807/tests/cefclient/cefclient_gtk.cc#L91-L98`.
- `libcef/browser/ui_thread.cc` L79-L80 calls `ui::SetMultiThreadedMessageLoopWayland()` when the Wayland Ozone backend is compiled in. (VERIFIED)
- `libcef/browser/views/window_impl.cc` L47-L49 and the input simulation paths (L54-L70, L702-L768) special-case Wayland, using `ui::test::EventGenerator` because `ui_controls` does not work there. This is for CEF's own test harness. (VERIFIED)
- CEF's own unit tests skip several checks on Wayland. Examples: `tests/ceftests/views/window_unittest.cc` L439-L453 (`IsMinimized()` not reported reliably), `tests/ceftests/views/textfield_unittest.cc` L296-L298 (EventGenerator key events "don't go through Wayland's text input protocol, so text input doesn't work"), and `tests/ceftests/views/button_unittest.cc` L625-L626 and L710-L726 (popups and activation). These are test-harness limits, not product limits, but they show where CEF's own Wayland coverage is thin. (VERIFIED)

### 1.2 Flags and platform selection

- `--ozone-platform=<name>` selects the platform. Chromium reads it in `ui/ozone/platform_selection.cc`. If the switch is missing, the first platform in the list is the default. (VERIFIED, `https://chromium.googlesource.com/chromium/src/+/refs/heads/main/ui/ozone/platform_selection.cc`)
- Auto-selection on Linux: `ui::SetOzonePlatformForLinuxIfNeeded()` in `ui/linux/display_server_utils.cc` L61-L90. If `--ozone-platform` is set, it does nothing. If exactly one of a reachable Wayland socket or a reachable X11 display exists, that one wins (L77-L79). If both or neither exist, `XDG_SESSION_TYPE=wayland` selects Wayland and anything else selects X11 (L70, L81-L89). (VERIFIED, `chromium/chromium` mirror, `ui/linux/display_server_utils.cc`)
- CEF calls that code. `libcef/common/chrome/chrome_main_delegate_cef.cc` L439-L479 overrides `PreSandboxStartup()` and calls `ChromeMainDelegate::PreSandboxStartup()` at L479. The Chromium call site is `chrome/app/chrome_main_delegate.cc` L1763-L1770. (VERIFIED)
- On Omarchy, XWayland runs, so a Wayland socket and an X11 display are both reachable. The session type then decides. Omarchy sets `XDG_SESSION_TYPE=wayland` (`default/hypr/envs.lua` L18, VERIFIED), so Wayland is the expected automatic choice (INFERRED. Runtime result UNKNOWN until run on Hyprland).
- Ordering: CEF's `OnBeforeCommandLineProcessing` call sits in `ChromeMainDelegateCef::BasicStartupComplete()` (`libcef/common/chrome/chrome_main_delegate_cef.cc` L424, VERIFIED). Chromium's content runner calls `BasicStartupComplete()` at `content/app/content_main_runner_impl.cc` L962 and `PreSandboxStartup()` at L1060 (VERIFIED). So an app's switch added in that hook is in place before platform selection (INFERRED from the call order, not run).
- Passing `--ozone-platform=wayland` explicitly from the app hook removes the dependence on environment detection. Use lowercase `wayland`. (INFERRED recommendation.)
- `--ozone-platform-hint=auto` and `--ozone-platform-hint=wayland`: a code search of `chromium/chromium` for `ozone-platform-hint` and `ozone_platform_hint` returned no hits. The switch is not in `ui/ozone/public/ozone_switches.cc`, which lists the Ozone switches (VERIFIED for the file, search-based for absence). Treat the hint as a no-op (INFERRED). Electron also removed its own `ELECTRON_OZONE_PLATFORM_HINT` in Electron 38 (`https://github.com/electron/electron/blob/main/docs/breaking-changes.md` L938-L940, VERIFIED).
- `--enable-features=UseOzonePlatform`: the only hits in Chromium are in test mixins (`testing/buildbot/mixins.pyl`, `infra/config/targets/mixins.star`). Do not use it in production (INFERRED from the search and the mixin usage).
- `docs/ozone_overview.md` in Chromium says Chrome on Linux "uses X11 Ozone backend by default" (L184-L186). The code in `display_server_utils.cc` above is newer and auto-selects Wayland under a Wayland session. Trust the code over the doc (VERIFIED discrepancy).
- Wayland IME switches (VERIFIED, `ui/ozone/public/ozone_switches.cc` L15-L24): `--enable-wayland-ime` and `--disable-wayland-ime`. The text-input protocol defaults to version 1. Version 3 (text-input-v3) is experimental and enabled with `--wayland-text-input-version=3`. Omarchy's Obsidian config already passes `--enable-wayland-ime` (`config/obsidian/user-flags.conf` L3, VERIFIED).

### 1.3 Windowed rendering (non-OSR)

- Views-hosted, top-level windows: supported (sections 1.1 and 1.2). (VERIFIED)
- Native parent (`CefWindowInfo::SetAsChild`): not supported on Wayland. The parent handle type on Linux is `cef_window_handle_t` = `c_ulong`, an X11 window ID (see `02-cef-rs.md` section 2.4, VERIFIED). The embedding work is open in CEF PR #4233 (see section 2.4).
- Off-screen rendering (OSR): cefclient's GTK path uses `GtkGLArea` on Wayland (`tests/cefclient/cefclient_gtk.cc` L110-L115, VERIFIED). cef-rs's own OSR setup forces `--ozone-platform=X11` and `--use-angle=gl-egl` when shared-texture OSR is on (`examples/tests_shared/src/browser/client_app_browser.rs` L143-L170, VERIFIED). That means OSR on Wayland currently runs through XWayland in cef-rs's examples. The NVIDIA reason is written in that code's comment.

### 1.4 Multiple BrowserViews in one CefWindow

- CEF's own `tests/cefclient/browser/views_window.cc` puts a second, ALLOY-style overlay BrowserView inside one window (L1055-L1066, cited in `02-cef-rs.md` section 3.3). (VERIFIED)
- Whether that works on Ozone/Wayland: UNKNOWN. No test or issue I read covers it. Experiment: two BrowserViews, one ALLOY and one CHROME runtime style, in a Views CefWindow on Hyprland with `--ozone-platform=wayland`. Check focus, resize, and context menus.

---

## 2. Known Wayland limits for a browser with a sidebar

### 2.1 Window decorations (VERIFIED for code, UNKNOWN for visible result)

- Hyprland always answers `xdg-decoration` with server-side mode. `CXDGDecoration::xdgModeOnRequestCSD` and `xdgModeOnReleaseCSD` both return `ZXDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE` (`src/protocols/XDGDecoration.cpp`, Hyprland main; functions `xdgDefaultModeCSD`, `xdgModeOnRequestCSD`). (VERIFIED)
- Omarchy adds a Hyprland titlebar plugin: package `omarchy-hyprland-titlebars` in `install/omarchy-base.packages`, and `default/hypr/workspace-modes.lua` L26 loads `/usr/lib/omarchy-hyprland-titlebars/titlebars.so`. (VERIFIED)
- Frameless CEF windows: CEF issue #4273 (open, filed against KWin, CEF 152.0.7) says frameless Views windows still request SSD, so the compositor draws a title bar on top of the app's own. PR #4274 "Fix frameless Views windows on Wayland" is open. (VERIFIED for the issue and PR state)
- Issue #4061 (closed 2026-10-10) says Weston and Mutter only support client-side decoration. Its comments say upstream Chromium changes from a contributor fixed it in M149, and a follow-up PR #4181 covers frameless shadows and corners. (VERIFIED from comments)
- On Omarchy, a frameless sidebar window could get the compositor's SSD path, plus the Omarchy titlebar plugin's bar. The visible result is UNKNOWN. Experiment: a frameless Views window and a normal Views window on Hyprland with `--ozone-platform=wayland`, with the titlebar plugin enabled and disabled. Record `WAYLAND_DEBUG=client` output for `set_mode`.

### 2.2 Multiple BrowserViews, popups, menus, tooltips

- Popups: Chromium uses `xdg_popup` for its own popups. CEF's test comments note popup behavior on Wayland (`button_unittest.cc` L170, VERIFIED). The embedding PR says popups fall back to `wl_subsurface` when the client has no `xdg_surface`, which can clip and does not dismiss on outside click (PR #4233 body, VERIFIED for the PR author's claim, applies only to embedding).
- Issue #3937 (select widgets crash on Wayland when clicked, closed 2025-05-29). Issue #3691 (status bubble position on Wayland, closed 2025-05-29). Both are fixed or closed. (VERIFIED titles and states)
- Context menus: a reported right-click placement bug on KDE 6.7 appears in the #2804 thread (comment by `mihawk90`, 2026-08-04, SECONDHAND user report). Not reproduced here.
- For a sidebar, popups anchored in the sidebar BrowserView should use `xdg_popup` on the top-level's surface. Not verified. UNKNOWN.

### 2.3 Drag and drop, clipboard

- Drag and drop: the PR #4233 author reports drag and drop both ways in a fork (issue #2804 comment, 2026-08-03). This is an author claim, not verified. Views-only D&D is UNKNOWN.
- Clipboard: CEF issue #3117 (open since 2021) reports a cefclient freeze when pasting from a GTK control into a browser text field. It is GTK-specific. Chromium's native Ozone clipboard path for Views is not verified (UNKNOWN).

### 2.4 Embedding (the reason Views is the only path)

- Issue #2804 "Linux: Add support for embedded Ozone/Wayland windows" is open. Last updated 2026-08-04. (VERIFIED)
- PR #4233 by `leandromqrs` (open, not draft, opened 2026-08-04, updated 2026-10-21) adds the native embed mode. Its own description says "this needs a decision from you before it can be finished... The API question is the blocker." It tested on Fedora 44 and GNOME 50 only. It touches `chromium` patch files and adds `cef_set_wayland_display`. (VERIFIED from PR body)
- A user comment on #2804 (linesight, 2026-08-04) says the maintainer's guideline expects several review rounds, possibly months, for a large upstream Chromium patch. I did not find that guideline text in a maintainer comment. (SECONDHAND)
- Maintainer comment `4594624031` (magreenblatt, 2026-06-01) on the same issue discusses ANGLE and the X11 embedding code. It does not state a review policy. (VERIFIED)
- So embedding is not available upstream. Views is the only supported windowing model, and the plan should assume it.

### 2.5 Fractional scaling and HiDPI

- Issue #2804 comments (2021) report blurry windows on HiDPI under XWayland. That is historical and XWayland-specific. (VERIFIED as user reports)
- Omarchy sets `xwayland { force_zero_scaling = true }` in `default/hypr/envs.lua` L42-L44 (VERIFIED). Any XWayland window on Omarchy therefore renders at scale 1 with no compositor scaling, which is INFERRED to make XWayland fallback paths look blurry or small on HiDPI displays. Relevant if the app falls back to `--ozone-platform=x11`, or to cef-rs's X11-forced OSR path (section 3).
- PR #4208 "linux: Fix oversized native window content on HiDPI displays" is open. (VERIFIED title and state)
- Native Wayland fractional-scale behavior in CEF Views: UNKNOWN. Experiment: run a Views window at 1.25 and 1.5 scale on a Hyprland output with `--ozone-platform=wayland`, and compare with `--ozone-platform=x11`.

### 2.6 GPU acceleration (Mesa and NVIDIA)

- cef-rs's comment on NVIDIA and DMABUF: shared-texture OSR must use `use-angle=gl-egl`, because the pixmap path works only with Mesa AMD and Intel drivers (`client_app_browser.rs` L143-L163, VERIFIED).
- CEF issue #3953 (closed 2026-01-25) made `use-angle=gl-egl` and `ozone-platform=x11` the default for Linux OSR shared textures. Issue #4237 (closed 2026-10-09) prefers native-handle SharedImages for OSR on NVIDIA. (VERIFIED titles and states)
- Omarchy's NVIDIA setup: `default/hypr/nvidia.lua` sets `NVD_BACKEND` and `LIBVA_DRIVER_NAME=nvidia` only when NVIDIA drives the display. It notes that forcing NVIDIA VA-API and GLX globally breaks Chromium video. Packages come from `install/hardware/nvidia.sh` (`nvidia-open-dkms`, `nvidia-utils`, `libva-nvidia-driver`). (VERIFIED)
- Windowed Views rendering on Ozone/Wayland with NVIDIA: UNKNOWN. Experiment: `chrome://gpu` and `vainfo` on the target machine, plus a 60-second WebGL and video playback run, on Mesa and NVIDIA.

### 2.7 Screen sharing (PipeWire and portals)

- WebRTC's default is `rtc_use_pipewire = is_linux && use_sysroot` (`third_party/webrtc/webrtc.gni` L121, VERIFIED). CEF's documented official build sets `use_sysroot=true` (`docs/automated_build_setup.md` L82, VERIFIED). So the compile flag may be on in CEF builds (INFERRED).
- Issue #4053 "Enable PipeWire desktop capture for Wayland" was closed 2025-12-08. The maintainer's comment says it "should be implemented upstream". I did not find a landed upstream change. Whether the PipeWire capturer is enabled at runtime is UNKNOWN.
- Arch's Chromium PKGBUILD sets `rtc_use_pipewire=true` (`gitlab.archlinux.org/archlinux/packaging/packages/chromium/-/raw/main/PKGBUILD` L356, VERIFIED). It lists `pipewire` as a build dependency (L71) and as an optional runtime dependency for "WebRTC desktop sharing under Wayland" (L79, VERIFIED). Helium's AUR package uses the same optional-dependency wording (VERIFIED).
- Portal: Omarchy's portal config routes `default=hyprland;gtk` and `Secret=gnome-keyring` (`config/xdg-desktop-portal/hyprland-portals.conf`, VERIFIED). The Hyprland portal backend's interface list was not read (the README does not list them). Screencast through the portal is UNKNOWN.
- Experiment: `getDisplayMedia` in a Views window on Hyprland. Check for the portal request, the picker, and the PipeWire node. Check `pw-dump` for a Chromium stream.

### 2.8 File chooser (xdg-desktop-portal)

- Chromium's `ui/shell_dialogs/shell_dialog_linux.cc` L11-L21 returns `SelectFileDialogLinuxPortal` when `USE_DBUS` is defined. Otherwise it asks the Linux UI, which may be null. (VERIFIED)
- CEF's `BUILD.gn` L1092-L1094 defines `USE_DBUS` only when `use_dbus` is true. The default for CEF's build is UNKNOWN from the files I read. (VERIFIED for the conditional, UNKNOWN for the default)
- CEF routes file dialogs through `ui::SelectFileDialog::Create` (`libcef/browser/file_dialog_manager.cc` L355-L359, VERIFIED).
- Omarchy's `default=hyprland;gtk` puts the GTK backend in the chain. `xdg-desktop-portal-gtk` is in `install/omarchy-base.packages` (VERIFIED). Which backend answers `FileChooser` on Omarchy is INFERRED, not verified. Experiment: an `<input type=file>` in a Views window, with `dbus-monitor` on `org.freedesktop.portal.FileChooser`.

### 2.9 Global shortcuts

- Omarchy's shortcuts are Hyprland binds (`default/hypr/bindings/*.lua`). They are not app-level global shortcuts. (VERIFIED for the existence of the binds)
- An app-level global shortcut needs the `org.freedesktop.portal.GlobalShortcuts` portal. Whether the Hyprland backend implements it: UNKNOWN from the README. Experiment: call `CreateSession` from a test client on Hyprland.

### 2.10 Notifications

- Omarchy's notification UI is a Quickshell plugin, `shell/plugins/notifications/Service.qml`, which names the `org.freedesktop.Notifications` interface (VERIFIED for the name, INFERRED that it serves it).
- Chromium's notification path on Linux was not traced in this pass. UNKNOWN.

### 2.11 IME

- Views text fields use the Wayland text-input protocol. Default version 1. Version 3 is experimental and needs `--wayland-text-input-version=3` (VERIFIED, `ozone_switches.cc` L21-L24).
- Real IME behavior (fcitx5 on Omarchy, which is installed by default per `install/omarchy-base.packages`) is UNKNOWN. Experiment: type Japanese or Chinese preedit into a Views text field under fcitx5 on Hyprland, with and without `--wayland-text-input-version=3`.

---

## 3. Does cef-rs expose what Wayland needs?

- `on_before_command_line_processing` is bound on the `App` trait in `cef/src/bindings/x86_64_unknown_linux_gnu.rs` L33689 (VERIFIED). cef-rs's example implements it at `examples/tests_shared/src/browser/client_app_browser.rs` L99-L175 (VERIFIED). A custom app can add `--ozone-platform=wayland` there. The call order is covered in section 1.2.
- Casing bug in cef-rs's example: when shared-texture OSR is on, `client_app_browser.rs` L165-L170 appends `--ozone-platform=X11` (uppercase) if no platform switch is set (VERIFIED). CEF's `ui/ozone/platform_selection.cc` compares the name against lowercase platform names and calls `LOG(FATAL)` on a miss (VERIFIED, CEF's Chromium source). CEF issue #4020 (closed 2025-10-23) reports exactly this crash, "Invalid ozone platform: X11", and says `x11` fixes it (VERIFIED). So cef-rs's example carries a latent crash on its OSR path. Use lowercase `x11` if that path is used. Whether the crash reproduces on 156 is UNKNOWN. (The cef-rs path was not run.)
- Linux window properties: `LinuxWindowProperties` has `wayland_app_id`, `wm_class_class`, `wm_class_name`, `wm_role_name` (`cef/src/bindings/x86_64_unknown_linux_gnu.rs` L1059-L1066, VERIFIED). CEF maps them in `libcef/browser/views/window_view.cc` L551-L556 (VERIFIED). cef-rs can set the Wayland app ID through this struct, and CEF's `cefclient` does the same in `tests/cefclient/browser/views_window.cc` L725 (VERIFIED).
- Parent window handle: the handle type is an X11 window ID on Linux (`02-cef-rs.md` section 2.4, VERIFIED). So cef-rs cannot embed in a Wayland surface. That matches CEF's own status.
- Views: `window_create_top_level`, `browser_view_create`, `panel_create`, `set_to_box_layout` are bound (see `02-cef-rs.md` section 3.1, VERIFIED). Nothing blocks the Views plan at the binding level.
- Sandbox: `cefsimple` sets `no_sandbox: !cfg!(feature = "sandbox")` (`examples/cefsimple/src/shared/mod.rs` L56, VERIFIED). The `cef` crate's default features include `sandbox` (`cef/Cargo.toml` L17, VERIFIED). cef-rs's bundler `cef/src/bin/bundle-cef-app/linux.rs` only calls `build_bundle`. No `chrome-sandbox` handling was found in `cef/src/build_util/*.rs` (search returned no hits, VERIFIED). So the bundler does not set SUID.
- cef-rs has no Wayland test or example (02 section 2.4, still true, VERIFIED by absence in CI workflow).

---

## 4. Other CEF apps and Electron on Wayland

- Electron: `--ozone-platform=x11|wayland|headless` is documented, and the default is automatic selection (`docs/api/command-line-switches.md` L205-L216, VERIFIED). Electron removed its own hint env var in 38 (see 1.2).
- Spotify (CEF-based): SECONDHAND. Community threads say the Linux client defaulted to XWayland for years and gained native Wayland slowly. A GNOME user reported a client-side title bar, and a Spotify engineer confirmed an XWayland-only regression in 1.2.86.502. Source: `community.spotify.com` Wayland support threads (search snippets only, not opened). Use only as direction.
- Issue #4061 cites Spotify as the downstream case for CSD on Weston and Mutter (VERIFIED, issue text).
- Steam: UNKNOWN. Not researched in this pass.
- Chromium itself: Omarchy's default Chromium ships `--ozone-platform=wayland` and `--ozone-platform-hint=wayland` in `config/chromium-flags.conf` (VERIFIED config). Whether those flags work as intended on Hyprland is UNKNOWN. This is the closest comparable with no CEF in the middle.
- OBS (CEF-based docks): issue #2804 shows OBS custom browser docks broken under Wayland, and OBS users used XWayland (SECONDHAND, user comments). Not opened.

---

## 5. Linux runtime and packaging

### 5.1 CEF binary contents (VERIFIED)

From `tools/make_distrib.py` L1487-L1510 (Linux branch) and `tools/distrib/linux/README.redistrib.txt`:

- Required: `libcef.so`, `icudtl.dat`, `v8_context_snapshot.bin`.
- Resources: `chrome_100_percent.pak`, `chrome_200_percent.pak`, `resources.pak`, `locales/` (`*.info` removed).
- SwiftShader for software ANGLE/Vulkan: `libvk_swiftshader.so`, `libvulkan.so.1`, `vk_swiftshader_icd.json`.
- `chrome_sandbox` is renamed to `chrome-sandbox` (`tools/make_distrib.py` L1490).
- `--ozone` builds add `libminigbm.so` (conditional) (`make_distrib.py`, VERIFIED).
- `cefsimple` and `cefclient` are sample binaries, not required.
- `BUILD.gn` L1089 adds `//third_party/angle:libEGL` as a dependency in one branch of the Linux target. I did not confirm whether the released archive ships `libEGL.so` or relies on the system copy. The cefclient OSR path loads `libEGL.so.1` at runtime (`cefclient_gtk.cc`, VERIFIED message text). UNKNOWN for the archive.

Read the file list from the real archive (`cef_binary_..._linux64.tar.bz2` on the index) before writing the PKGBUILD. Index entries for 156.0.7 stable and beta exist for `standard`, `minimal`, `client`, and `tools` (VERIFIED from `index.json`).

### 5.2 Sandbox (VERIFIED for packaging practice, UNKNOWN for the Arch default)

- CEF's SUID instructions: `cmake/cef_macros.cmake.in` L185-L194 prints `chown root:root` and `chmod 4755` for `chrome-sandbox` (VERIFIED).
- Arch's Chromium package installs `chrome-sandbox` with mode 4755 at `/usr/lib/chromium/chrome-sandbox` (PKGBUILD L449, VERIFIED).
- `ungoogled-chromium-bin` does `chown root` and `chmod 4755` in `package()` (AUR PKGBUILD L97-L98, VERIFIED).
- `cef-vaapi` installs `chrome-sandbox` as 4755 at `/usr/lib/cef/chrome-sandbox` (AUR PKGBUILD L565, VERIFIED).
- `cef-vaapi-bin` repackages a prebuilt tarball and does not set the mode (VERIFIED).
- Helium's AUR package does not set SUID in the PKGBUILD I read. (VERIFIED, reason UNKNOWN)
- Whether Chromium on Arch prefers unprivileged user namespaces over the SUID helper: the Chromium sandboxing doc was not fetched successfully. UNKNOWN. Experiment: `zgrep CONFIG_USER_NS /proc/config.gz` and run the app with the SUID bit removed.
- Running with `no_sandbox` is not an option for a browser. cefclient sets `settings.no_sandbox = true` only when `CEF_USE_SANDBOX` is not defined (`tests/cefclient/cefclient_gtk.cc` L119-L124, VERIFIED).

### 5.3 Runtime libraries

- Build hosts: CEF documents "Ubuntu 20.04+, Debian 10+" for Linux builds (`docs/branches_and_building.md` L86, VERIFIED). That is the build host, not the runtime glibc floor.
- Runtime glibc: INFERRED to be at least the glibc the binary was linked against. Arch is newer, so no problem. Confirm with `objdump -T libcef.so | grep GLIBC_` on the downloaded archive (UNKNOWN until run).
- Arch's Chromium `depends` list (PKGBUILD L20-L40): `alsa-lib`, `at-spi2-core`, `cairo`, `dbus`, `desktop-file-utils`, `expat`, `glib2`, `glibc`, `gtk3`, `hicolor-icon-theme`, `libcups`, `libffi`, and more (VERIFIED). A CEF app needs a similar subset, plus `nss` and `libva` for video.
- `cef-vaapi-bin` depends on `alsa-lib`, `at-spi2-core`, `dbus`, `libdrm`, `libffi`, `libpulse`, `libva`, `libxcomposite`, `libxdamage`, `libxkbcommon`, `libxrandr`, `mesa`, `nss`, `pango` (VERIFIED, AUR PKGBUILD L11-L25).

### 5.4 PKGBUILD shape

Based on the sources above, a package for this app needs:

- The app binary, linked against `libcef.so` and `libcef_dll_wrapper`. Subprocesses run the same binary, because the sample apps call `execute_process` at start-up (cef-rs `examples/cefsimple/src/shared/mod.rs` L40, VERIFIED). So Linux needs no separate helper bundle (INFERRED for the app).
- `libcef.so`, `icudtl.dat`, `v8_context_snapshot.bin`, the `.pak` files, `locales/` (only the locales shipped), and the SwiftShader libraries, under `/usr/lib/<app>/`.
- `chrome-sandbox` at `/usr/lib/<app>/chrome-sandbox`, mode 4755, owner root.
- A launcher that sets `--ozone-platform=wayland` and the app ID.
- A `.desktop` file with `StartupWMClass` or Wayland app ID matching `wayland_app_id`.
- `depends=(` the runtime set above `)`, plus `pipewire` as an optional dependency for screen sharing and `xdg-desktop-portal-hyprland` and `xdg-desktop-portal-gtk` as optional dependencies.

Packaging examples that exist: `cef-vaapi-bin` (prebuilt CEF repackaged), `cef-vaapi` (source-built CEF with SUID), `ungoogled-chromium-bin` and `helium-browser-bin` (Chromium-based, not CEF). (VERIFIED from AUR PKGBUILDs)

---

## 6. Verdict

**Go, conditionally, for CEF Views on Wayland as the first Linux target. No-go for native embedding of CEF into a Wayland surface.**

Views on Ozone/Wayland is the upstream-supported path, and CEF's own tests run on it. Embedding is not available and not near. Build the sidebar as a second BrowserView in one top-level Views window, and do not depend on embedding.

Top risks:

1. Hyprland forces server-side decoration (VERIFIED). A frameless sidebar window may get a compositor or plugin title bar. Test before choosing frameless.
2. Screen sharing via PipeWire and the portal is unverified (UNKNOWN). The upstream flag exists only under `use_sysroot` (VERIFIED), and the capturer enablement is unknown.
3. IME: text-input v1 by default. Real IME with fcitx5 is untested (UNKNOWN).
4. NVIDIA: the OSR path on Wayland runs through XWayland in cef-rs's examples. Windowed Views on NVIDIA is untested (UNKNOWN).
5. Sandbox packaging: a setuid `chrome-sandbox` is needed, or the sandbox must work through user namespaces. cef-rs's bundler does not handle it (VERIFIED). The Arch default is UNKNOWN.
6. Upstream churn: the embedding PR and frameless fixes are open. A version bump may change behavior (VERIFIED from PR states).

Smallest experiments that settle the open questions (all on Hyprland, Mesa and NVIDIA):

- E1. cef-rs `cefsimple` or a cefclient build from the 156 `standard` archive, with `--use-views --ozone-platform=wayland`. Records start-up, window, and `WAYLAND_DEBUG=client` output. Settles section 1 and the decoration result.
- E2. Two BrowserViews (ALLOY and CHROME) in one Views window. Settles multi-view focus and resize.
- E3. Frameless window with and without the Omarchy titlebar plugin. Settles section 2.1.
- E4. `getDisplayMedia` in a Views window, plus `pw-dump`. Settles screen sharing.
- E5. An `<input type=file>` with `dbus-monitor` on the FileChooser portal. Settles the file chooser.
- E6. fcitx5 Japanese preedit in a Views text field, with and without text-input v3. Settles IME.
- E7. `chrome://gpu`, `vainfo`, and a WebGL and video run on Mesa and NVIDIA. Settles GPU.
- E8. `--ozone-platform` auto versus explicit, with `XDG_SESSION_TYPE=wayland` and no switch. Settles the auto-detection result.
- E9. Fractional scale 1.25 and 1.5 under Wayland and X11 (XWayland). Settles HiDPI.

What could not be verified:

- Any runtime behavior on Hyprland, Mesa, or NVIDIA. Nothing was executed on a Wayland machine.
- The magpcss.org CEF forum. Search did not return a forum thread. The Level1Techs and Collabora claims are SECONDHAND. One maintainer quote came through a summary of the Level1Techs thread, not from the page.
- Chromium's `docs/linux/sandboxing.md`, which did not fetch. The userns versus SUID order is unverified.
- Omarchy's Chromium behavior under real use. The repo shows configuration, not outcomes.
- Steam's Wayland state and Spotify's current state beyond the SECONDHAND snippets.
- Whether the released CEF archive ships `libEGL.so`.
- Whether Chromium's Ozone code ever reads the `OZONE_PLATFORM` environment variable that Omarchy sets (`default/hypr/envs.lua` L17). I did not find it in the code search I ran. UNKNOWN.
