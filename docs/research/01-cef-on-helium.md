# 01. Can CEF build on a Helium-patched Chromium?

Accessed: 2026-10-23. Read-only research. No project code written, no builds run.

Pinned sources (commit or tag, so citations stay stable):

- CEF: `chromiumembedded/cef` master, commit `7f8a5ec28e932aa62b583c8248e3df2e4dfe4807` (2026-10-20). CEF branch 8078 pins a different tag (section 1).
- Helium: `imputnet/helium` main, commit `f29a25968e483f8bb85e47829ccd487bbe229199` (2026-10-21). `imputnet/helium-chromium` main is the same commit (VERIFIED, `gh api`).
- Helium platform repos: `imputnet/helium-macos` (submodule `helium-chromium` pointing at `imputnet/helium`) and `imputnet/helium-linux`. Both GPL-3.0 (VERIFIED, `gh api` license field).
- Chromium: tags `156.0.8078.47` (Helium pin) and `156.0.8078.48` (CEF branch 8078 pin, Chromium stable per chromiumdash).

Labels:

- **VERIFIED**: read in a primary source (CEF, Helium, Chromium, or a live API).
- **SECONDARY**: read in a document written by someone else. Here that is ardia's doc, in a local checkout at `~/Developer/arctos/ardia`. Not checked against a build.
- **INFERRED**: my reasoning from verified facts.
- **UNKNOWN**: not found, with the experiment that would settle it.

Local working copies (read-only, outside the repo): `/tmp/yaac-research-2026-10-23/cef` and `/tmp/yaac-research-2026-10-23/helium`. Helper scripts: `/tmp/yaac-scripts-2026-10-23/overlap.py` and `hunks.py`. Ardia's CEF-era doc: `/tmp/yaac-research-2026-10-23/ardia-cef-source-build.md` (from `git -C ~/Developer/arctos/ardia show 5cbcb8db^:docs/cef-source-build.md`).

---

## Summary

- CEF master pins Chromium `156.0.8078.0`. CEF branch 8078 pins `156.0.8078.48`. Helium pins `156.0.8078.47`. All three are milestone 156 (VERIFIED).
- CEF applies its own patches with `git apply -p0 --ignore-whitespace` on a plain Chromium checkout or tarball, and aborts on any failure (VERIFIED). Its tooling accepts a different Chromium tag and a source tarball. So a Helium-patched tree is mechanically possible.
- A precedent exists (SECONDARY). Ardia's CEF-era build (CEF branch 8037 on Chromium 154.0.8037.58, with ungoogled-chromium 154.0.8037.57-1, same milestone) ran CEF's patcher first and ungoogled-chromium second. It recorded real conflicts and reverted some ungoogled patches that it said broke CEF. Ardia removed CEF in October 2026 when it cut over to a direct Chrome-framework build.
- Concrete collisions:
  - **GN-time RLZ conflict on macOS and Windows.** CEF forces `enable_rlz=true`. Helium's `disable-rlz` sets `enable_rlz_support=false`. Chromium's own assert rejects that combination (VERIFIED from three sources; section 6.3). Ardia's CEF-era build left the offending hunk out, and its later Helium-era build reverts it (SECONDARY). Either way RLZ code stays in the build.
  - **Helium flags vs CEF (unconfirmed).** Helium sets `safe_browsing_mode=0` and `enable_mdns=false`. Ardia reports that patches assuming those values break CEF (SECONDARY). I could not confirm a concrete break in Chromium or CEF source (section 6.3).
  - **Patch overlap.** 96 Chromium files are touched by both patch sets. 32 have overlapping hunk ranges (INFERRED, not applied). Many are in the Chrome UI layer.
- Milestone cadence: Helium's merges are about two weeks apart recently. CEF pins even milestones only (VERIFIED; section 1).
- Recommended order: CEF first, Helium second. Ardia used this order (SECONDARY). The decision still needs experiment E1 (section 7).

---

## 1. Chromium branch and lag

- **VERIFIED.** CEF master pins `refs/tags/156.0.8078.0` in `CHROMIUM_BUILD_COMPATIBILITY.txt` (commit `a26ddfd27`, 2026-10-07). Earlier master pins: 154.0.8037.0 (2026-09-06), 152.0.7977.0 (2026-08-19), 151.0.7922.0 (2026-07-08), 150.0.7871.0 (2026-06-02). Source: `https://github.com/chromiumembedded/cef/commits/master/CHROMIUM_BUILD_COMPATIBILITY.txt`.
- **VERIFIED.** Branch 8078 pins `refs/tags/156.0.8078.48` (`gh api repos/chromiumembedded/cef/contents/CHROMIUM_BUILD_COMPATIBILITY.txt?ref=8078`). Its latest commit is `426920c33`, 2026-10-22, "Update to Chromium version 156.0.8078.48 (#4323)". Branch 7977 (152) pins `refs/tags/152.0.7977.169`. Release branches track point releases. Master does not (it sits at `.0`).
- **VERIFIED.** CEF branch table, `https://chromiumembedded.github.io/cef/branches_and_building` (WebFetch, 2026-10-23): `8078` = Chromium 156, labelled "Beta"; `7977` = 152, "Extended"; `7871` = 150, "LTC"; older rows "Legacy (unsupported)". No "Stable" row. The page says "CEF is currently tracking even-numbered Chromium milestone branches" and "Updating CEF branches is currently a manual process so there will likely be a delay".
- **VERIFIED, conflict.** The CEF builds index, `https://cef-builds.spotifycdn.com/index.json` (downloaded 2026-10-23), lists `156.0.7+g47dbc1d+chromium-156.0.8078.17` as channel `stable` (last_modified 2026-10-22, macosarm64 and linux64). The same index lists 156 builds as `beta` from 2026-10-08. The wiki labels 156 Beta. Which label is current is UNKNOWN.
- **VERIFIED.** Chromium stable: chromiumdash `fetch_releases` lists `156.0.8078.48` and `156.0.8078.47` with the same timestamp, epoch-ms 1792526229680 (2026-10-20T19:57Z). The page does not label the timestamp, so read it as an INFERRED release date. Earliest 156 point release in the fetched list: `156.0.8078.35`, 2026-10-14. The chromiumdash schedule page rendered no data, so Chromium 157 and 158 stable dates are UNKNOWN.
- **VERIFIED.** Helium pin: `chromium_version.txt` = `156.0.8078.47`, `revision.txt` = `1` (`https://github.com/imputnet/helium/blob/f29a259/chromium_version.txt`). Helium commit history for that file: 152.0.7977.75 (2026-09-01), 153.0.8010.36 (09-08), 153.0.8010.52 (09-17), 154.0.8037.57 (09-22), 154.0.8037.97 (10-02), 155.0.8059.39 (10-06), 155.0.8059.84 (10-15), 156.0.8078.47 (10-20).
- **Point-release gap (VERIFIED arithmetic).** As of 2026-10-23: Chromium stable `.48`, Helium `.47` (one point release behind), CEF branch 8078 `.48`, CEF master `.0`, and the most recent CEF binary in the index `.17` (about 31 point releases behind stable). The index binary predates the branch pin. Security fixes ride point releases, so the binary lag matters more than the milestone match.
- **Milestone cadence (VERIFIED dates; INFERRED rate).** Helium's first merge per milestone: 152 on 2026-09-01, 153 on 09-08, 154 on 09-22, 155 on 10-06, 156 on 10-20. Intervals of 7, 14, 14 and 14 days. Helium's merge date is a proxy for the Chromium stable date, since the chromiumdash schedule was unavailable. CEF's pins came earlier: 152 on 08-19, 154 on 09-06, 156 on 10-07, that is 13 to 16 days before Helium merged the same milestone. So recent milestones arrive about every two weeks (INFERRED), and CEF pins even milestones only, about every four weeks or more. CEF's even-milestone pins have led Helium's merges.
- **INFERRED.** CEF's 156 branch update (2026-10-07) came before 156 stable (about 2026-10-14). CEF tracks the milestone's branch point, so the lag to stable is measured in point releases, not milestones.

---

## 2. Build integration: how CEF consumes a Chromium tree

### 2.1 Pipeline (VERIFIED, `tools/automate/automate-git.py`, commit 7f8a5ec)

1. Reads the Chromium pin from `CHROMIUM_BUILD_COMPATIBILITY.txt`. `--chromium-checkout` overrides it (lines ~1438-1447).
2. `chromium_version = chromium_checkout.split('/')[2]` (line ~1481). `--chromium-url` defaults to `https://chromium.googlesource.com/chromium/src.git` (lines ~1501-1506).
3. Either `gclient sync` the checkout, or extract `--chromium-archive` (a tarball) and apply CEF's `tarball_deps` and `tarball_gclient` patches (lines ~1534-1546).
4. Verifies the tree. For an archive, it compares the Chromium version file to `chromium_version` (lines ~1559-1565). For a git checkout, it checks `is_git_checkout`.
5. Applies CEF patches via `tools/patcher.py`, called from `apply_patch` (automate-git.py lines ~541-552; `apply_patch_file` in patcher.py lines ~32-49).
6. `patcher.py` runs `git apply -p0 --ignore-whitespace` (`tools/git_util.py`, line ~175). On any failure it writes an ERROR note and `sys.exit(1)` (patcher.py lines ~86-105).
7. The caller runs `subprocess.check_call` (automate-git.py `run()`, lines ~176-181), so a failed patch aborts automate-git.

### 2.2 Patch sets (VERIFIED)

- CEF: `patch/patch.cfg` lists patches by name. `patch/patches/` holds **118** `.patch` files. `patch/README.txt` says `tools/patch_updater.py` updates them when Chromium changes. `CHROMIUM_UPDATE.txt` lists the Chromium files that need CEF follow-up when they change.
- Helium: `patches/series` lists **346** active quilt entries, grouped as `helium/`, `ungoogled-chromium/`, `inox-patchset/`, `brave/`, `iridium-browser/`, `bromite/`, `upstream-fixes/`, `debian/`.

### 2.3 Surface overlap (VERIFIED from patch text; INFERRED for conflict risk)

Script: `/tmp/yaac-scripts-2026-10-23/overlap.py`.

- CEF patches touch **462** distinct Chromium paths. The Helium series touches **1645**.
- **96 exact paths** are touched by both. **81 directories** are shared.
- Examples (not exhaustive):
  - `BUILD.gn` and build config: CEF `angle_commit_config`, `gn_config`, `v8_build`; Helium `disable-rlz`, `fix-building-without-mdns-and-service-discovery`.
  - `chrome/browser/safe_browsing/BUILD.gn`: CEF `chrome_browser_safe_browsing.patch`; Helium `inox-patchset/fix-building-without-safebrowsing.patch` and `ungoogled-chromium/fix-building-without-safebrowsing.patch`.
  - `components/gcm_driver/gcm_client_impl.cc`: CEF `components_gcm_disable.patch`; Helium `ungoogled-chromium/disable-gcm.patch`. Same intent, overlapping hunk (CEF old lines 296-302; Helium 296-334).
  - `components/crash/core/app/crashpad*.cc`, `chrome/common/crash_keys.cc`: CEF `crashpad_1995.patch`; Helium `helium/core/crash-reporter-privacy-tweaks.patch`, `crash-reporting-prefs.patch`.
  - `components/webui/version/*`: CEF `chrome_browser_webui_version.patch`; Helium `helium/core/add-helium-versioning.patch`.
  - `chrome/chrome_paks.gni`, `chrome/chrome_repack_locales.gni`: CEF `gn_config.patch`; Helium `remove-unused-ai-resources`, `remove-description-snippet-deps`.
  - `chrome/browser/ui/views/frame/*`, `toolbar/*`, `location_bar/*`, `ui/app_menu_model.*`: CEF `chrome_runtime_views.patch`, `chrome_browser_browser.patch`; Helium `helium/ui/layout/*`, `tab-search-in-toolbar`, `zen-mode`, `location-bar`, `app-menu-model`.
  - `content/browser/renderer_host/*`, `third_party/blink/*`: CEF `browser_security_policy_1081397`, `webkit_move_pip`, `blink_ax_viewport_collapse`; Helium `helium/core/noise/core.patch`, `shift-right-click-context-menu`.
- **INFERRED.** Hunk-level: I compared old-line hunk ranges (padding 3 lines) for the 96 shared files. **32 files** have overlapping ranges (`hunks.py`). CEF patches were written against `156.0.8078.0` and Helium against `.47`, so the line numbers are not directly comparable. Treat this as a rough upper bound, not an apply test.
- **SECONDARY, corroborating.** Ardia's ungoogled status (`engine/patches/ungoogled-status.tsv`, ardia checkout) marks `disable-gcm` partial because "CEF already stubs `GCMClientImpl::Start`", and marks `add-flag-to-show-avatar-button` and `add-flag-for-incognito-themes` as conflicting with CEF's toolbar and frame patches. These match the overlaps above.

### 2.4 Feasibility of the two stacking orders

- **CEF first, Helium on top.** Standard `automate-git.py` with `--chromium-checkout=refs/tags/156.0.8078.47` (VERIFIED flag; using `.47` against CEF's `.0` pin is an untested override). Then apply Helium's quilt series. Ardia used this order (SECONDARY, section 3).
- **Helium first, CEF on top.** Use `--chromium-archive` with a tarball that already has Helium's series applied (VERIFIED mechanism, lines ~1537-1565). Then CEF's patcher runs on that tree. Any CEF patch that no longer applies aborts the build (VERIFIED, section 2.1). Each conflicting CEF patch must be re-diffed by hand (INFERRED).
- **Ardia's warning about order (SECONDARY).** Its doc says that once ungoogled and domain substitution "have changed the context of CEF's own patches, CEF's patcher can no longer recognize them and stops". So CEF's patch step must run before other patches touch those files. Later GN-arg changes must regenerate args, not re-run the CEF patch step. This matches the verified fail-on-error behaviour and argues for CEF first.
- **Which is smaller depends on the conflict count.** The 96-file overlap is symmetric. Helium's set is about three times larger by path count. Neither order is shown to be cheaper without an apply test (UNKNOWN; experiment E1).

---

## 3. Version skew and precedent

- **VERIFIED.** CEF is on even milestones only at present (CEF wiki). The wiki table still lists odd branches 151 and 149 as Legacy, with no 153 or 155 rows. Helium merged every milestone from 152 to 156 (section 1).
- **INFERRED.** On odd milestones (157, 159, ...) CEF has no branch. A CEF build on Chromium N would be unsupported CEF-on-N, or we run N-1 with a one-milestone skew. With milestones about two weeks apart (section 1), that skew lasts until the next even pin, roughly two to four weeks (INFERRED).
- **SECONDARY. Ardia's precedent (CEF + ungoogled-chromium, same milestone).** From `git -C ~/Developer/arctos/ardia show 5cbcb8db^:docs/cef-source-build.md`, the CEF-era build doc:
  - Tree: CEF branch 8037 at `154.0.28+g564dd6c`, Chromium `154.0.8037.58`, ungoogled-chromium `154.0.8037.57-1` (not Helium, which is ungoogled-based). Same milestone; no skew.
  - Stacking order: step 2, CEF's patcher, translator and `gn gen`; step 3, ungoogled-chromium series with exceptions; step 4, domain substitution, then the app's own series (`engine/patches/series`).
  - Conflicts recorded: `disable-gcm` (CEF stubs the same function); `0021-disable-rlz` (CEF requires `enable_rlz`; kept partial, "RLZ stays inert: there's no brand code, and its host is substituted"); `add-flag-to-show-avatar-button` and `add-flag-for-incognito-themes` (CEF's toolbar and frame patches); `enable-extra-locales` (breaks CEF's string packs); `fix-building-without-safebrowsing` and `fix-building-without-mdns-and-service-discovery` (reverted: "they assume `safe_browsing_mode=0` or disabled mdns, which break CEF"). See section 6.3 on whether that holds.
  - Timeline (from the doc and commit `5cbcb8db`, `git -C ~/Developer/arctos/ardia show --stat 5cbcb8db`): CEF's native side left the repo with the cutover on 2026-10-01. Their CEF-specific patches (`cef-*.patch`) were removed 2026-10-08. The doc was renamed and marked CEF-free on 2026-10-09.
  - Reason given for dropping CEF: the app's own layer (ArdiaCore) builds Chrome's `//chrome:chrome_framework` directly and no longer needs CEF's API or Alloy/OSR. The doc does not record a CEF-specific build failure as the reason.
  - Measured build cost on their machine (Apple M5 Pro, shared, from the doc's step table): sync ~40 min and 30 GB; CEF patch step 2 min; first `chrome_framework` build about 2 h (libcef not measured separately); about 43 GB disk in total.
  - Verify: the doc, the commits above, `engine/patches/ungoogled-status.tsv`, and `engine/patches/chromium-helium-rlz-for-cdm-storage-id.patch`. The ardia build was not re-run here.
- **Precedent for CEF + Helium directly: none found.** Two WebSearch queries and `gh search repos ungoogled` / `gh search code` returned no public project that combines them (UNKNOWN; search coverage limited). Ardia's doc is the only precedent found, and it is a local checkout, not a published source.
- **Other, not read in full.** ungoogled-chromium's build doc says it is not self-contained for all platforms (WebSearch snippet, gogs mirror). A FreeBSD port (`www/cef`, freshports) builds CEF from Chromium and CEF sources with an extra patch set (snippet only). Verify before relying on either.

---

## 4. Runtime styles

- **VERIFIED.** `include/internal/cef_types_runtime.h` (lines 39-48) describes two styles: Chrome style "provides the full Chrome UI and browser functionality"; Alloy style "provides less default browser functionality but adds additional client callbacks and support for windowless (off-screen) rendering". Lines 64-74 define `CEF_RUNTIME_STYLE_DEFAULT` ("Use the default style. See above documentation for exceptions."), `CEF_RUNTIME_STYLE_CHROME` and `CEF_RUNTIME_STYLE_ALLOY`. Windowless rendering always uses Alloy (line 49). The header does not say which style DEFAULT resolves to (UNKNOWN). Set CHROME explicitly.
- **VERIFIED.** `docs/architecture.md` (lines 49, 131-138): Alloy = Chromium content layer; Chrome = full Chrome UI layer. "The Chrome runtime ... adds implementations for many advanced features (including datalist/autofill, extensions, gamepad, webhid, webmidi, etc.), along with standard Chrome UI toolbars and dialogs ... At the time of writing, the Chrome runtime requires use of the Views framework ... and does not support off-screen rendering." That text is from an older era, so it is weak evidence of the current state.
- **VERIFIED.** Many APIs are annotated "Only used with Chrome style" or "Only supported with Chrome style", e.g. `include/internal/cef_types.h` lines ~481, ~495, ~741, ~1507, ~1534, ~1544; `include/cef_command_handler.h` lines 59, 73, 86.
- **VERIFIED.** Both `libcef/browser/alloy/` and `libcef/browser/chrome/` exist at master `7f8a5ec`. No deprecation or removal marker found in the headers, `docs/architecture.md`, `docs/general_usage.md` (grep for alloy, runtime style, deprecat: no matches), or the General Usage wiki page.
- **Status.** Both are present in master source. Deprecation status beyond that is UNKNOWN. CEF has no GitHub releases to check (`gh api repos/chromiumembedded/cef/releases` returned 0 entries, VERIFIED), and I found no separate release-notes page.
- **For tabs, extensions and chrome:// pages, Chrome runtime is the one that matters** (`docs/architecture.md`, above). Chrome runtime requires Views and does not support OSR, so the browser must be windowed.
- **INFERRED.** Helium's UI patches (`helium/ui/*`) target the same browser UI (`chrome/browser/ui/views/*`) that CEF's Chrome-runtime patches (`chrome_runtime_views.patch`) modify. That layer is where the two stacks collide.

---

## 5. Build cost

- **VERIFIED.** CEF's own quick start, `https://chromiumembedded.github.io/cef/master_build_quick_start` (fetched 2026-10-23), says: at least 16 GB RAM (32 GB+ recommended), 150 GB free disk for a **Debug** build, and about 4 hours on a 100 Mbps connection with a 2.4 GHz, 16-logical-core SSD machine. Stated for both Windows and macOS.
- **VERIFIED, conflict.** CEF's branches-and-building wiki says "at least 8GB of system memory". The quick start says 16 GB. Both are CEF pages. Treat 16 GB as the safer number.
- **VERIFIED.** Chromium Linux docs (`https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/linux/build_instructions.md`, lines 13-19): x86-64, at least 8 GB RAM (more than 16 GB "highly" recommended), 32 GB or 16 GB swap for 8 GB or 16 GB RAM, at least 100 GB free disk. No build time.
- **VERIFIED.** Chromium mac docs (`mac_build_instructions.md`, lines 13-16, 92-99): Intel or Arm Mac, Xcode, macOS SDK. Git cache about 30 GB for Chromium; clone "30 minutes on even a fast connection, and many hours on slower ones". `mac_arm64.md` line 62: builds "tend to take a long time" (no figure).
- **VERIFIED.** Helium: `helium-macos/docs/building.md` lines 10-13: macOS 12+, Xcode 26. `./build.sh` downloads prepared archives unless `-c` (clone Chromium, line 61). No RAM, disk or time figures. `helium-linux/README.md` lines 54-55: Helium's CI builds "within hours". That is CI, not local hardware.
- **SECONDARY.** Ardia's doc (section 3): Apple M5 Pro, shared machine. Sync ~40 min, 30 GB. First `chrome_framework` build about 2 h. Incremental builds 20 s to 2 min. About 43 GB disk in total. No libcef figure.
- **UNKNOWN.** Arm64 Linux hardware and build time, and a measured time for a CEF Chrome-runtime build on our target machines. Experiment E1 would measure it.

---

## 6. De-Googling and CEF build flags

### 6.1 Helium's GN args (VERIFIED, `flags.gn` at commit f29a259)

`enable_reporting=false` (L7), `enable_widevine=true` (L9), `google_api_key=""` (L11), `safe_browsing_mode=0` (L14), `enable_mdns=false`, `enable_service_discovery=false`, `enable_remoting=false`, `disable_fieldtrial_testing_config=true`, `use_official_google_api_keys=false`, `exclude_unwind_tables=true`, `chrome_pgo_phase=0`, `treat_warnings_as_errors=false`. `helium-macos/flags.macos.gn` L12 and `helium-linux/flags.linux.gn` L1: `is_official_build=true`.

### 6.2 CEF's required GN args (VERIFIED, `tools/gn_args.py` at 7f8a5ec)

- All platforms: `optimize_webui=True`, `enable_widevine=True` (L368), `clang_use_chrome_plugins=False`.
- macOS and Windows: `enable_cdm_host_verification=True`, `enable_cdm_storage_id=True` (L376-377), `enable_rlz=True` (L380), with the comment "Enable use of the RLZ library as required by CDM storage ID". `alternate_cdm_storage_id_key` is set at L320.
- Linux: `enable_linux_installer=False` (L387).
- CEF's `BUILD.gn` enforces these with `assert` (lines 255-266). For `is_mac || is_win`: `assert(enable_cdm_host_verification)`, `assert(enable_cdm_storage_id)`, `assert(alternate_cdm_storage_id_key != "")`, `assert(enable_rlz)`.

### 6.3 Conflicts

- **RLZ (VERIFIED from source).**
  - Chromium's own `rlz/buildflags/buildflags.gni` at tag `156.0.8078.47` (fetched from googlesource 2026-10-23): line 9 `enable_rlz_support = is_win || is_apple || is_chromeos`; lines 11-13 `declare_args() { enable_rlz = is_chrome_branded && enable_rlz_support }`; lines 15-16 `assert(!enable_rlz || enable_rlz_support, "RLZ is only supported on Windows, Apple and ChromeOS.")`.
  - Helium's `patches/inox-patchset/disable-rlz.patch` (series line 7) changes the `buildflags.gni` hunk to `enable_rlz_support = false` (patch line 27). It also removes the RLZ targets from `gn_all`.
  - CEF forces `enable_rlz=true` on macOS and Windows (`gn_args.py` L380), and asserts it (`BUILD.gn` L261-265).
  - **Result (INFERRED from the three sources; not run).** On macOS and Windows, `enable_rlz=true` with `enable_rlz_support=false` violates Chromium's own assert at GN time. No config satisfies both unless one of them is patched. Linux is not affected: `enable_rlz_support` is false there and CEF does not force `enable_rlz`.
  - **Ardia's handling (SECONDARY, two cuts).** In its CEF-era build, `0021-disable-rlz` was `partial`: it left out the `buildflags.gni` hunk (`engine/patches/ungoogled-status.tsv` line 7). In its later Helium-era build, `engine/patches/chromium-helium-rlz-for-cdm-storage-id.patch` reverts that hunk. Stated reason for the later one: Widevine's CDM storage id (`enable_cdm_storage_id`) hashes RLZ's machine id on Mac, so reverting keeps every user's storage ids and licences unchanged. Ardia's doc also says "RLZ stays inert: there's no brand code, and its host is substituted." Both cuts leave RLZ code in the build. Keeping it is a privacy trade-off to decide, not a free fix.
- **Safe Browsing and mDNS (reported by ardia; not confirmed).** Helium sets `safe_browsing_mode=0` and `enable_mdns=false`. Ardia reverted its `fix-building-without-safebrowsing` and `fix-building-without-mdns-and-service-discovery` patches, and skipped its two safe-browsing-mode fixes, saying they "assume `safe_browsing_mode=0` or disabled mdns, which break CEF" (SECONDARY). What I checked:
  - mDNS. `libcef/browser/media_router/media_sink_impl.cc` includes `chrome/browser/media/router/discovery/mdns/cast_media_sink_service_impl.h` (VERIFIED, line 10). In Chromium 156.0.8078.47, `chrome/browser/media/router/discovery/BUILD.gn` lists the `mdns/*` sources, including `cast_media_sink_service_impl.cc/.h`, with no `enable_mdns` gate in the fetched file (VERIFIED). `enable_mdns` is consumed elsewhere in Chromium, e.g. `services/network/host_resolver.cc` (GitHub code search hit, not read in full). Helium's mDNS patch removes a `service_discovery_sniffer` dependency from `gn_all` and edits `mdns/dns_sd_device_lister.cc`, `dns_sd_registry.cc/.h` (VERIFIED from the patch). It does not gate the cast sink CEF includes. **Not confirmed as a break.**
  - Safe browsing. Helium's `inox-patchset` and `ungoogled-chromium` `fix-building-without-safebrowsing.patch` edit `chrome/browser/safe_browsing/BUILD.gn`, `components/safe_browsing/core/browser/*`, `components/safe_browsing/content/common/safe_browsing.mojom` and others (VERIFIED from the patch). CEF's `libcef/browser/net_service/proxy_url_loader_factory.cc` includes `components/safe_browsing/core/common/safebrowsing_constants.h` and uses `safe_browsing::kCustomCancelReasonForURLLoader` and `kNetErrorCodeForSafeBrowsing` (VERIFIED, lines 24, 1253-1254). The patch does not appear to touch `core/common`, but I did not verify that exhaustively. **Not confirmed as a break.** Experiment E6 settles it.
- **is_official_build.** Helium sets `is_official_build=true`. CEF's defaults are `False` (`tools/gn_args.py` ~L246-248). Not checked whether CEF overrides it on release builds.
- **Widevine.** `flags.gn` sets `enable_widevine=true`, which CEF also requires. A third-party review snippet says Helium "doesn't include Widevine". The primary source (`flags.gn`) wins for the flag. Whether the shipped binary includes it is UNKNOWN.
- **Reporting and API keys.** CEF's `gn_args.py` does not set `enable_reporting`, `google_api_key` or `safe_browsing_mode`, so Helium's values pass through unchanged.
- **Component build.** CEF requires `is_component_build=False` for distribution builds (`gn_args.py` ~L289). Helium does not set it. No conflict found.

---

## 7. Unknowns and experiments that would settle them

- **E1 (decisive): apply both stacks to one tree.** Run CEF's `automate-git.py --no-build --chromium-checkout=refs/tags/156.0.8078.47`, then apply Helium's `series` in order. Count rejects. Disk budget about 150 GB (CEF quick start, Debug build). Time it. Answers section 2.4.
- **E2: RLZ on macOS.** Build with Helium's series plus a reverting patch for the `buildflags.gni` hunk. Check that Chromium's `buildflags.gni` assert and CEF's `BUILD.gn` assert both pass. Check whether CDM storage works with RLZ kept on. Only the first part is INFERRED now.
- **E3: archive mode.** Produce a Helium-patched tarball, feed it via `--chromium-archive` with `--chromium-archive-sha256`, and confirm CEF's version check and tarball patches pass.
- **E4: CEF 156 channel.** Resolve the stable-vs-beta conflict for 156 from a CEF release announcement or forum post. UNKNOWN now.
- **E5: odd-milestone plan.** Ask CEF maintainers whether they will return to odd branches. UNKNOWN now.
- **E6: Helium GN flags vs CEF.** Build CEF with Helium's `flags.gn` values (`safe_browsing_mode=0`, `enable_mdns=false`) and record the first errors. Settles the unconfirmed claim in section 6.3.
- **Not found:** CEF release notes (no GitHub releases exist; no separate page found); Chromium 157 and 158 stable dates; any public CEF + Helium or CEF + ungoogled build besides ardia's local one; arm64 Linux build requirements.

---

## Verdict

**Feasible with caveats.** A CEF build against a Helium-patched Chromium is mechanically possible. CEF's tooling accepts a different tag and a source tarball, and applies its own patches on top. It is not a drop-in. The GN-time RLZ conflict on macOS and Windows has a precedent (ardia), and keeping RLZ code in the build is the cost. Helium's `safe_browsing_mode=0` and `enable_mdns=false` are reported by ardia to break CEF; I could not confirm that in the source I checked. Odd Chromium milestones have no CEF branch.

**Recommended integration order: CEF first, Helium second, confirmed by E1.** Reasons: (1) CEF's patcher aborts on failure and stops once other patches change the context of its files (VERIFIED, plus ardia's SECONDARY report), so it must run first and must not be re-run after other patches. (2) Ardia used this order on the same milestone (SECONDARY), so it is the only order with a precedent. (3) A CEF build on a Chromium tag that CEF already supports gives a working baseline before we add Helium's 346 patches. Helium-first via `--chromium-archive` is equally mechanical, but each CEF patch would then need a hand re-diff. The reject count from E1 should decide the order, not this note.

**Expected maintenance burden per Chromium upgrade (INFERRED, partly from ardia's SECONDARY steps):**

- Helium's merges arrive about every two weeks (section 1), so the rebase work below recurs at about that rate. CEF's even-milestone pins come roughly two weeks before Helium's merges, which gives a window to rebase CEF first.
- Revert domain substitution, update the ungoogled/Helium checkouts, re-sync with the new CEF branch, then re-run both patch stacks (ardia's steps 1-5).
- Re-resolve the 96-file overlap (32 hunk-level collisions at today's base; the count grows).
- Rebase CEF's patch set when CEF's branch moves. CEF's update is manual and can lag stable. On odd milestones there is no branch.
- Rebase Helium's series.
- Re-check the RLZ and CDM asserts, the Helium-vs-CEF flags, and the GN args on every bump.
- Re-sync and rebuild cost, from ardia (SECONDARY): about 40 min and 30 GB for sync, then about 2 h for a first `chrome_framework` build on an M5 Pro. libcef not measured.

**Top 3 risks:**

1. **RLZ and Helium's GN settings.** RLZ is a GN-time conflict on macOS and Windows; keeping RLZ code is a privacy trade-off. Helium's `safe_browsing_mode=0` and `enable_mdns=false` are reported by ardia to break CEF builds. I could not confirm that, so it is a risk to test (E6), not a fact. E2 and E6 come before we commit to Helium's flag set.
2. **Shared Chrome UI surface.** 96 shared files, 32 with overlapping hunks at today's base. Ardia's own conflicts (avatar button, incognito themes, toolbar and frame) fall in the same area. Helium's UI patches (`helium/ui/*`) sit in the same layer as CEF's Chrome runtime. Each Chromium bump multiplies rebase work.
3. **Patch-stack fragility and skew.** CEF's patcher aborts on any failed patch and stops once other patches change its context (verified, plus ardia). CEF branches only even milestones, so odd milestones mean an unsupported CEF or a skew of about two to four weeks. Ardia removed CEF in October 2026 when it cut over to a direct Chrome-framework build (SECONDARY). That is a data point on long-run maintenance, not a technical failure.

**What is unverified:**

- The CEF 156 stable-vs-beta label (section 1).
- What `CEF_RUNTIME_STYLE_DEFAULT` resolves to (section 4).
- Arm64 Linux build requirements.
- Chromium 157 and 158 stable dates.
- The E1 apply test (no tree has been built or patched).
- The safe-browsing and mDNS breakage ardia reports (section 6.3; E6).

**Also note (not a technical risk):** Helium's `AGENTS.md` (`https://github.com/imputnet/helium/blob/f29a259/AGENTS.md`) states that imputnet/helium does not accept AI-assisted patches. It is addressed to AI agents, and I did not act on it, since this task does not involve Helium code. For our own patches intended upstream, a human must write and understand them. CEF's `AI_POLICY.md` (`https://github.com/chromiumembedded/cef/blob/7f8a5ec/AI_POLICY.md`) permits AI assistance with human accountability.
