# E2: cefclient on Omarchy under Hyprland (Wayland)

Date: 2026-10-23. Machine: `toni@omarchy` (Arch Linux, Omarchy 4.0.0.alpha, kernel 7.1.8-arch1, Intel Iris Plus with Mesa 26.2.5). Session: Hyprland 0.56.2 on eDP-1 (2560x1600 physical, scale 1.25). Work directory on that machine: `/home/toni/yaac-spike/e2/`. Nothing was installed, removed, or upgraded. No sudo was used.

Labels: VERIFIED means observed in a run, a file, or command output. INFERRED means reasoning from verified facts. NEEDS USER means a check that needs someone at the screen. OPEN means not settled by this experiment.

## Summary

1. cefclient starts on the live Hyprland session with `--use-views --ozone-platform=wayland` and without `--no-sandbox`. Renderer processes run in a user-namespace sandbox with a seccomp filter. The chrome-sandbox helper is not setuid and no SUID helper was used. (VERIFIED)
2. With `XDG_SESSION_TYPE=wayland` and no ozone switch, Chromium selects native Wayland. `--ozone-platform=x11` runs through XWayland. (VERIFIED)
3. `OZONE_PLATFORM=wayland` in the environment is not read by Chromium in the tested case. (VERIFIED for one case)
4. The window negotiates server-side decoration. The client sent `set_mode(2)` and the compositor answered `configure(2)`. Hyprland's core does not draw titlebars, and no Hyprland plugin was loaded. The expected result is no titlebar, with Hyprland's default 1 px border. (VERIFIED for the protocol and plugin list, INFERRED for the titlebar expectation, because the Hyprland docs check did not complete). The visual result is NEEDS USER.
5. The cefclient content never appeared in the captures. Details and the open cause are in the "Open issue" section. (VERIFIED for the observation, OPEN for the cause)
6. The GPU process maps Mesa EGL, Mesa gallium, and the Intel VA-API driver. No SwiftShader library is mapped. The GL backend and the `chrome://gpu` table were not read. (VERIFIED for the maps, NEEDS USER for the page)
7. The GPU process runs with `Seccomp: 0` and `NoNewPrivs: 1`, while renderers run with `Seccomp: 2`. I did not determine whether that is expected for this build. (VERIFIED as observed, reason UNKNOWN)
8. Unexpected finding: a system file, `/usr/share/chromium/extensions/fcoeoabgfenejglbffodgkkbkcdhcgfn.json`, made a fresh CEF profile install the "Claude in Chrome" extension in two runs, and it opened a "Sign in - Claude" window in one of them. `--disable-extensions` stopped this in every later run. This matters for yaac. (VERIFIED)

## Versions and hashes

| Item | Value |
|---|---|
| CEF build | 156.0.7+g47dbc1d+chromium-156.0.8078.17, channel `stable` in index.json. The same version is also listed as `beta`. The stable entry was used. |
| Chromium | 156.0.8078.17 |
| Build index | `https://cef-builds.spotifycdn.com/index.json`, sha1 `7da9dfc3b43980661c9ac58f531d3170b402b4ee`, 10542669 bytes, downloaded 2026-10-23 |
| Standard archive | `cef_binary_156.0.7+g47dbc1d+chromium-156.0.8078.17_linux64.tar.bz2`, 695545603 bytes, sha1 `7cc0c6f80e671332e00f981f5059a9567c02f5bc` (matches index, VERIFIED) |
| Client archive | `cef_binary_..._linux64_client.tar.bz2`, 331469629 bytes, sha1 `8b9656a34a83963a6cab865e17d7f1fd0a3b4292` (matches index, VERIFIED). It contains `cefsimple` and no `cefclient`. |
| Built cefclient | `build-cefclient/tests/cefclient/Release/cefclient`, sha1 `cd495f9dc1d38cc21e4192226463eb78a82a8279` |
| Build tools | cmake 4.4.4, ninja 1.13.2, gcc 16.2.1 |
| Runtime packages | Hyprland 0.56.2, mesa 1:26.2.5-1, nss 3.131.1-1, gtk3 1:3.24.52-2, libva 2.24.1-1, libglvnd 1.7.0-3, libpipewire 1:1.6.9-1 |
| System Chromium | `chromium` 155.0.8059.84-1 is installed. It was not used. |

Build commands, with an out-of-tree build directory and `-j2` to protect RAM on the daily-driver machine:

    cmake -G Ninja -S cef-standard/cef_binary_156.0.7+g47dbc1d+chromium-156.0.8078.17_linux64 -B build-cefclient -DCMAKE_BUILD_TYPE=Release -DPROJECT_ARCH=x86_64
    ninja -j2 cefclient

The build finished with 288 steps and no errors (VERIFIED, `logs/ninja-cefclient.log`). It printed a manual SUID command for `chrome-sandbox`. I did not run it, because it needs sudo.

## Session environment for GUI runs

Plain SSH shows `XDG_SESSION_TYPE=tty`, so the runs export the values from `systemctl --user show-environment`:

- `XDG_RUNTIME_DIR=/run/user/1000`
- `WAYLAND_DISPLAY=wayland-1`
- `HYPRLAND_INSTANCE_SIGNATURE` from `/run/user/1000/hypr/`
- `XDG_SESSION_TYPE=wayland`, except in runs 2c and 2d, which set `tty` on purpose
- `DISPLAY=:0` (XWayland, socket `/tmp/.X11-unix/X0`)
- `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`

Every run also passed `--cache-path=<run dir>/cache --password-store=basic`. This kept the profile inside e2 and kept the keyring out of the test. The wrapper is `/home/toni/yaac-spike/e2/scripts/run-gui.sh`. It runs cefclient under `timeout 25`, reads `hyprctl clients -j` and `monitors -j` at 9 s (or 13 s), and optionally captures a crop with grim. No input was sent and `hyprctl dispatch` was not used.

## Results table

| Run | Command (extra flags) | Exit | Observed |
|---|---|---|---|
| r1 | `--use-views --ozone-platform=wayland --url=chrome://sandbox`, `WAYLAND_DEBUG=client` | not recorded (older wrapper) | Window class `cef`, app_id `cef`, title "Sandbox Status", `xwayland:false`, `floating:false`. Decoration request and reply in the trace. The grim capture hung on `ext_image_copy_capture` and was killed. No PNG. |
| r1b | same, no WAYLAND_DEBUG | 137 | Screenshot skipped by a bug in my wrapper (jq matched two windows). A second window with cefclient's pid, titled "Sign in - Claude - Chromium", was in the 9 s snapshot. See the extension finding. |
| r1c | same, no WAYLAND_DEBUG | 137 | Screenshot captured at crop 515,30 507x622. No cefclient content in the crop. See the open issue. |
| r2a | `--use-views --ozone-platform-hint=auto --url=about:blank --disable-extensions` | 124 | `xwayland:false`. Native Wayland. |
| r2b | `--use-views --ozone-platform=x11 --url=about:blank --disable-extensions` | 124 | `xwayland:true`. X11 through XWayland works. |
| r2c | `OZONE_PLATFORM=wayland`, `XDG_SESSION_TYPE=tty`, no ozone switch | 124 | `xwayland:true`. The env var did not change the choice. |
| r2d | control for r2c: `XDG_SESSION_TYPE=tty`, no env var, no switch | 124 | `xwayland:true`. Same as r2c. |
| r2e | control for r2a: no ozone flag at all, Wayland session | 124 | `xwayland:false`. Same as r2a, so the hint flag made no visible difference here. |
| r4 | `--use-views --ozone-platform=wayland --url=chrome://gpu --disable-extensions`, process and GPU capture at 13 s, one crop | 124 | Title "GPU Internals", `xwayland:false`. Process status and GPU maps recorded. The crop is pixel-identical to r1c (md5 `d4a41e6a8977cc15fb6512e72688a4e7`). |
| r3 | window decoration | n/a | From the r1 protocol trace and `hyprctl clients -j`, below. |
| r5 | popups, context menu, text input, clipboard | not run | These need input, which the run rules forbid. Clipboard was excluded on purpose. NEEDS USER if wanted. |
| r6 | ldd, NEEDED, and package owners | n/a | Below. |

Exit 124 means `timeout` sent SIGTERM at 25 s and cefclient exited on it. Exit 137 means cefclient did not exit on SIGTERM and had to be killed (r1b, r1c). Run r1 predates the exit-status line in the wrapper. The cause of the difference is UNKNOWN.

Full logs are in `/home/toni/yaac-spike/e2/runs/<label>/stderr.log` on the Omarchy machine. Lines seen in every run (VERIFIED): `wayland_surface.cc:170` and `:185` (server lacks some surface protocols), `sandbox_linux.cc:405` ("InitializeSandbox() called with multiple threads in process gpu"), `dbus/xdg/portal.cc:126` ("Failed to register with org.freedesktop..."), and `idle_linux.cc:111` (no D-Bus screensaver service).

## Run 1: sandbox mode

- `chrome-sandbox` at `build-cefclient/tests/cefclient/Release/chrome-sandbox` has mode 755 (`-rwxr-xr-x`) and owner `toni:toni`. It is not setuid. (VERIFIED, `stat`)
- `kernel.unprivileged_userns_clone` is 1, and `/proc/sys/user/max_user_namespaces` is 63006. (VERIFIED)
- Process status at 13 s in r4, from `/proc/<pid>/status` for processes owned by toni:
  - Renderers: `NSpid` lists three values, which means a nested PID namespace. `Seccomp: 2`, `NoNewPrivs: 1`.
  - Storage utility: `Seccomp: 2`. Network utility: `Seccomp: 0`.
  - GPU process: `NoNewPrivs: 1`, `Seccomp: 0`.
  - No `chrome-sandbox` process appears in the tree.
  - The renderers therefore run in a user-namespace sandbox with a seccomp filter. (VERIFIED)
- No sandbox FATAL or "Failed to move to new namespace" text appeared in any run. The only sandbox text is the `sandbox_linux.cc:405` warning listed above.

## Run 2: platform selection

- Automatic choice with an XDG session of `wayland` and both sockets reachable: Wayland (r2e). (VERIFIED, settles the auto-detect question in note 04)
- `--ozone-platform-hint=auto` alone gave Wayland (r2a), the same as r2e. The hint looks like a no-op in this case. (VERIFIED for the result, INFERRED that the hint has no effect)
- `--ozone-platform=x11` gave XWayland (r2b). (VERIFIED)
- With session type `tty` and `OZONE_PLATFORM=wayland` in the environment, the result was X11 (r2c), the same as the control (r2d). Chromium did not read `OZONE_PLATFORM` in this case. (VERIFIED for this case)
- Uppercase flags were not tested, because the run rules forbid them.

`xwayland` in `hyprctl clients -j` is the signal used to tell the platform apart. The Chromium log does not name the platform in a form I could match.

## Run 3: window decoration

- Fields in `hyprctl clients -j` for the cefclient window: `address`, `class` (`cef`), `initialClass`, `title`, `pid`, `floating`, `fullscreen`, `xwayland`, `mapped`, `hidden`, `at`, `size`, `workspace`, `monitor`, `pinned`, `grouped`. There is no `decorated` field, and the decoration mode is not reported. (VERIFIED)
- Protocol trace in r1 (`WAYLAND_DEBUG=client`):
  - `xdg_toplevel.set_app_id("cef")`
  - `zxdg_toplevel_decoration_v1.set_mode(2)`, which is server-side. (VERIFIED)
  - `zxdg_toplevel_decoration_v1.configure(2)` twice from the compositor. (VERIFIED)
  - `set_title("")`, then `set_title("Sandbox Status")`.
  - `set_window_geometry` 950x713, then 512x625.
- Compositor side, read-only: `hyprctl plugin list` reports "no plugins loaded". The Omarchy titlebar plugin that note 04 says `workspace-modes.lua` loads is not loaded in this session. (VERIFIED)
- Read-only options: `decoration:active_opacity` is 1.0 and `general:border_size` is 1. `grep` of `~/.config/hypr/` finds no titlebar or `xdg-decoration` setting. (VERIFIED)
- Expected result: no titlebar from either side, and a 1 px border. Core Hyprland draws borders, shadows, and rounding. Titlebars come from plugins. (INFERRED. I tried to confirm this in the Hyprland wiki, but the pages were too large to read in the fetch, so the check is not complete.)
- The visual result is NEEDS USER. The captures did not show the cefclient window.
- Frameless windows were not tested.

## Run 4: GPU

- The GPU process was alive at 13 s. (VERIFIED)
- Its mapped libraries include `libEGL.so.1.1.0`, `libEGL_mesa.so.0.0.0`, `libgallium-26.2.5-arch1.1.so`, `libgbm.so.1.0.0`, and `iHD_drv_video.so` (Intel VA-API driver). No `libvk_swiftshader` was mapped. (VERIFIED, `/proc/<gpu>/maps`)
- `libgallium` holds both the Intel and the software Gallium drivers, so the maps alone do not separate hardware GL from software GL. The likely answer is hardware through Mesa Iris. (INFERRED)
- `chrome://gpu` loaded (title "GPU Internals"), but its text was not captured. The GL backend and feature status are NEEDS USER.
- NVIDIA was not tested. (UNKNOWN)

## Open issue: cefclient content never reached the screen

What the captures show. The r1c and r4 crops are pixel-identical (md5 `d4a41e6a8977cc15fb6512e72688a4e7`), so they show the same static image taken about 6 minutes apart. The image is the desktop background and a dark input box with the text "Enter Pa..." near the lower right of the crop. The box is not cefclient content as far as I can tell. I did not identify its source.

What the r1 trace shows (no lock was active at that time, per the journal):

- `wl_surface#34` attached two SHM buffers (512x625) at 05:56:10.28 and 05:56:10.31 UTC.
- One frame callback was done at 05:56:10.31.
- One presentation feedback was reported `discarded()` at 05:56:10.33. A second feedback had no outcome before teardown.
- Chromium sent no further commits before the run ended at about 25 s.

What the Omarchy journal shows (local time is UTC+3, and the journal logs in UTC):

- The idle monitor went idle at 08:59:04 and active at 08:59:14. Nothing else happened until 09:04:33.
- A lock-timeout started `omarchy-system-lock` at 09:06:57.
- r1c ran at about 09:01 and r4 at about 09:07. So no lock event falls before r1c, and the r4 capture came about 30 s after the lock started.
- The journal does not show whether the lock stayed on screen. The later `hyprctl layers` listing, taken after the runs, showed no lock surface.

Conclusion. The cefclient window did not present its content in these runs. The r1 trace shows a discarded presentation with no lock active, which points to a paint or presentation problem in the CEF window. The static "Enter Pa..." box and the matching capture from r1c do not fit a lock screen, because no lock event preceded r1c. The cause is OPEN. Please confirm on screen what you see, and whether a password prompt or lock screen appeared around 09:01 and 09:07 local time. A next experiment would be a longer delay with a capture while you watch, and a run with `--disable-gpu-compositing`.

## Extension finding (affects yaac)

- `/usr/share/chromium/extensions/fcoeoabgfenejglbffodgkkbkcdhcgfn.json` contains `{ "external_update_url": "https://clients2.google.com/service/update2/crx" }`. (VERIFIED)
- `pacman -Qo` reports no package owns that file. I did not create it, and its origin is UNKNOWN. Please check whether you installed it, for example with the Claude desktop app. (VERIFIED for no owner)
- In r1b and r1c, CEF installed the extension into the run profile as `Default/Extensions/fcoeoabgfenejglbffodgkkbkcdhcgfn/1.0.102_0`. The manifest name is "Claude" and the description is "Claude in Chrome". (VERIFIED)
- In r1b, a window titled "Sign in - Claude - Chromium" with cefclient's pid was in the 9 s snapshot. The log has "Can't find manifest for native messaging host com.anthropic.claude_browser_extension". The link between that window and the install is INFERRED.
- The first run reached `clients2.google.com` through the external update URL. (INFERRED from that URL and the installed extension)
- With `--disable-extensions` (r2a to r4), no extra window appeared and no extension directory was created. (VERIFIED)
- The system Chromium package reads the same directory. I did not test that. (INFERRED)
- I deleted the two downloaded extension copies from the run caches. The downloaded archives remain in e2.

For yaac, a CEF app on a machine like this picks up external-extension entries. The app should pass `--disable-extensions` or otherwise block them, and not depend on the system default.

## Side effects outside e2

- `~/.config/chromium` has no file newer than the e2 index download, so the user's system Chromium profile was not modified. (VERIFIED)
- `~/.pki/nssdb` does not exist, so nothing was written there. (VERIFIED)

## Note 04 UNKNOWNs

| Note 04 item | Status | Basis |
|---|---|---|
| 1.2 Auto-selection with `XDG_SESSION_TYPE=wayland` | Settled: Wayland | r2e |
| 1.2 `--ozone-platform-hint` is a no-op | Consistent with a no-op, tested in one config | r2a vs r2e |
| 1.2 Chromium reads the `OZONE_PLATFORM` env var | Settled for this case: not read | r2c vs r2d |
| 1.1 Views window works on Ozone/Wayland | Partly. The window maps and gets configured. Its content did not reach the screen in these captures. OPEN. | r1, r1c, r4 |
| 1.3 Native parent (windowed) embedding | Not tested | n/a |
| 1.4 Two BrowserViews in one window | Open, not tested | n/a |
| 2.1 Server-side decoration on Hyprland | Protocol settled (SSD). Expected no titlebar, INFERRED. Visual NEEDS USER. Frameless OPEN. | r1 trace, r3 |
| 2.2 Popups and menus | NEEDS USER (needs input) | not run |
| 2.3 Drag and drop, clipboard | Not run. Clipboard excluded on purpose. | n/a |
| 2.5 Fractional scale (1.25 here) | OPEN, not run | monitor scale 1.25 seen |
| 2.6 GPU on Intel Mesa | Partly. Mesa EGL and VA-API loaded, no SwiftShader. GL backend NEEDS USER. | r4 maps |
| 2.6 NVIDIA | Not tested (UNKNOWN) | n/a |
| 2.7 Screen sharing (PipeWire, portal) | OPEN. `libpipewire-0.3.so.0` is named in libcef (dlopen). Runtime use not tested. | strings, r6 |
| 2.8 File chooser | Not run | n/a |
| 2.9 Global shortcuts | Not run | n/a |
| 2.10 Notifications | Not run | n/a |
| 2.11 IME (fcitx5) | NEEDS USER (needs typing) | n/a |
| 5.1 Does the archive ship `libEGL.so`? | Settled: no. libcef loads `libEGL.so.1` and `libGLESv2.so.2` from the system (libglvnd). | `Release/` listing, strings, r4 maps |
| 5.2 Arch sandbox default | Partly settled. With no setuid helper, the userns sandbox works on this kernel. SUID-present behavior not tested. | r4 |
| 5.3 glibc floor | Settled: libcef needs GLIBC 2.25. cefclient built here needs GLIBC 2.38. | `objdump -T` |
| 5.4 PKGBUILD runtime list | Drafted below from DT_NEEDED | r6 |

## Run 6: dependencies (for a PKGBUILD, nothing installed)

- `ldd` on cefclient and libcef.so reports zero "not found" lines each. (VERIFIED)
- cefclient DT_NEEDED: `libcef.so`, `libX11.so.6`, `libgtk-3.so.0`, `libgdk-3.so.0`, `libgdk_pixbuf-2.0.so.0`, `libgobject-2.0.so.0`, `libglib-2.0.so.0`, `libstdc++.so.6`, and glibc. Packages: `libx11`, `gtk3`, `gdk-pixbuf2`, `glib2`, `libgcc`, `glibc`. (VERIFIED)
- libcef.so DT_NEEDED maps to these packages: `glibc`, `glib2`, `nspr`, `nss`, `at-spi2-core` (libatk, libatk-bridge, libatspi), `dbus`, `libcups`, `libx11`, `libxcomposite`, `libxdamage`, `libxext`, `libxfixes`, `libxrandr`, `mesa` (libgbm), `expat`, `libxcb`, `libxkbcommon`, `cairo`, `pango`, `systemd-libs` (libudev), `alsa-lib`, and `libgcc`. (VERIFIED)
- dlopen names in libcef strings: `libEGL.so.1` and `libGLESv2.so.2` (`libglvnd`), `libvulkan.so.1` (`vulkan-icd-loader`), and `libpipewire-0.3.so.0` (`libpipewire`). The strings also name `libgtk-3.so.0` and `libgtk-4.so.1`. `libgtk-4.so.1` is not in DT_NEEDED. (VERIFIED for strings, INFERRED that it is optional)
- Draft PKGBUILD depends for this build: `nss`, `nspr`, `gtk3`, `glib2`, `at-spi2-core`, `dbus`, `libcups`, `libx11`, `libxcomposite`, `libxdamage`, `libxext`, `libxfixes`, `libxrandr`, `mesa`, `libxcb`, `libxkbcommon`, `cairo`, `pango`, `systemd-libs`, `alsa-lib`, `expat`, and `gdk-pixbuf2`. Optional: `libpipewire` (screen capture), `libglvnd` and `vulkan-icd-loader` (GL and Vulkan at runtime), and the xdg-desktop-portal packages for file dialogs. (INFERRED from DT_NEEDED and the dlopen strings. Not verified by a package build.)
- glibc floor: `GLIBC_2.38` for cefclient built here. (VERIFIED)

## Files

- Results and screenshots: `/Users/tonibergholm/Developer/yaac-spike-results/e2-cefclient-hyprland.md` (this file), `e2-r1-views-wayland-sandbox.png` (run r1c crop, no cefclient content visible), and `e2-r4-gpu-views-wayland-crop.png` (run r4 crop, pixel-identical to r1c). Neither crop shows cefclient content.
- On Omarchy: `/home/toni/yaac-spike/e2/` with `index.json`, the three archives, `cef-client/` and `cef-standard/` (extracted), `build-cefclient/`, `logs/`, `runs/<label>/` (stderr, clients.json, window.json, pids.txt, procs.txt, gpu-maps.txt, screenshots), and `scripts/`. It takes several GB. Delete it when the spike is done.

## Limits

- No input was sent, so no interactive behavior was tested.
- The only GPU was Intel. NVIDIA was not tested.
- Only the cefclient sample was run.
- Screen content was checked only through the crops above. Nothing else on the screen was captured.
