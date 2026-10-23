# 03. Chrome extensions, uBlock Origin, and Manifest V2 under CEF

Status: research notes, read-only. Accessed 2026-10-23 unless a line says otherwise.

Tags used below:

- VERIFIED: read in the cited source (file, commit, or fetched page).
- INFERRED: follows from verified facts, not tested.
- UNKNOWN: not determined. The last section lists the experiment that would settle it.

Version anchors. The Chromium `main` mirror on GitHub is Chromium 158 (`chrome/VERSION` reads `MAJOR=158`, `BUILD=8120`, VERIFIED, https://github.com/chromium/chromium/blob/main/chrome/VERSION). CEF master's `CHROMIUM_BUILD_COMPATIBILITY.txt` pins `refs/tags/156.0.8078.0` (VERIFIED, https://github.com/chromiumembedded/cef/blob/master/CHROMIUM_BUILD_COMPATIBILITY.txt). Helium's `chromium_version.txt` pins `156.0.8078.47` (VERIFIED, https://github.com/imputnet/helium/blob/main/chromium_version.txt). Where a Chromium file was read at 156, the tag is cited on googlesource. Otherwise the citation names `main` and the file is marked INFERRED for 156.

## Summary

- Chrome removed MV2 for users in July 2025 (Chrome 138 was the last version that could run it). The Web Store removed the remaining MV2 extensions on 2026-08-31.
- The Chromium source still contains MV2 code. One function, `ShouldDisableLegacyExtensions()`, hard-codes "disable" unless a test-only flag is set. Helium, Brave, and ungoogled-chromium all change that function to return false.
- CEF removed its public extension API in 2024 (`CefExtension`, `CefExtensionHandler`, `CefRequestContext::LoadExtension` are gone from master). CEF's Chrome runtime still runs Chromium's extension system, and Chrome-style windows can open `chrome://extensions`.
- uBO MV2 should work under CEF Chrome style (INFERRED, not yet run), but it needs the same Chromium patch that the forks use. The patch is one function. Maintenance is ongoing.

## 1. Chromium MV2 status

### Chrome's timeline

VERIFIED. Source: https://developer.chrome.com/docs/extensions/develop/migrate/mv2-deprecation-timeline (fetched 2026-10-23).

- 2022-01 and 2022-06: the Web Store stops accepting new MV2 extensions.
- 2024-06-03: phase-out starts on Beta, Dev, and Canary.
- 2024-10-09: installed MV2 extensions begin to be disabled on stable. Enterprises using `ExtensionManifestV2Availability` are exempt.
- 2025-03-31: MV2 is off by default, but users can still re-enable it. The page names Chrome 138 as "the final version of Chrome to support Manifest V2 extensions" when paired with the policy.
- 2025-07-24 (Chrome 138): MV2 is disabled on all channels, and users can no longer turn it back on. The page says the policy "will be removed with Chrome 139", and MV2 extensions "will cease to function for any user upgrading to Chrome 139 and subsequent versions".
- 2026-08-31: all remaining MV2 extensions are removed from the Chrome Web Store. Installs on Chrome 138 or earlier keep running but cannot update.

The page names no Chrome flag. It names one policy, `ExtensionManifestV2Availability`.

### What the Chromium source does now

VERIFIED, read from `chromium/chromium` at `main` (Chromium 158) and tag `156.0.8078.47` (googlesource: https://chromium.googlesource.com/chromium/src/+/refs/tags/156.0.8078.47/extensions/browser/manifest_v2_handler.cc). The 156 and 155 (`155.0.8059.39`) files have the same function body.

- `extensions/browser/manifest_v2_handler.cc:35`: `bool g_allow_mv2_for_testing = false;`
- `extensions/browser/manifest_v2_handler.cc:111-117`: `ShouldDisableLegacyExtensions()` returns `true` unless `g_allow_mv2_for_testing` is set. That is the single gate.
- `extensions/browser/manifest_v2_handler.cc:157` and `:171`: `ShouldBlockExtensionInstallation()` and `ShouldBlockExtensionEnable()` both call the gate first.
- `extensions/browser/manifest_v2_handler.cc:345`: `ScopedTestMV2Enabler` sets `g_allow_mv2_for_testing` through a `base::PassKey`. Declared in `extensions/browser/manifest_v2_handler.h:31` and `:102-104`. Test-only.
- `extensions/browser/manifest_v2_handler.cc:136-137`: pref `mv2_deprecation_unsupported_ack_globally`.
- `extensions/browser/manifest_v2_handler.h` (enum `MV2ExtensionState`): `kUnaffected`, `kSoftDisabled`, `kUserReEnabled`, `kOther`, `kHardDisabled`.
- Call sites that enforce the gate: `chrome/browser/extensions/extension_service.cc:628` (enable), `chrome/browser/extensions/standard_management_policy_provider.cc` (two sites), `extensions/browser/api/management/management_api.cc` (a `CHECK`), `extensions/browser/api/webstore_private/extension_install_status.cc` (install). Source: https://github.com/chromium/chromium/blob/main/chrome/browser/extensions/extension_service.cc

Component exemption. `extensions/browser/manifest_v2_util.cc:37-40` returns "not affected" for component locations, with the comment "Ignore component extensions (they're implementation details of Chrome)". `Manifest::IsComponentLocation()` covers `kComponent` and `kExternalComponent` (`extensions/common/manifest.h:88-91` at 156). So the gate never touches a component-location extension. A built-in component is therefore the route that avoids the gate. Helium takes that route (section 2.1). Whether that was Helium's reason is INFERRED.

Manifest parsing. `extensions/common/extension.cc` supports manifest versions 2 and 3 (`kMinimumSupportedManifestVersion = 2`). MV2 parses without any switch. The switch `--allow-legacy-extension-manifests` (`kAllowLegacyExtensionManifests`, `extensions/common/switches.cc:15-16`, consulted at `extension.cc:128-131`) only admits manifest versions below 2. It does not re-enable MV2. This is an easy point to get wrong.

Policy. `components/policy/resources/templates/policy_definitions/Extensions/ExtensionManifestV2Availability.yaml` is still in the tree. It says `deprecated: true` and `supported_on: chrome.*:110-138`. The description still says "Manifest v2 extensions support will be deprecated". `policy_atomic_groups.yaml` still lists it. `DeviceLoginScreenExtensionManifestV2Availability` (device policy) also still exists in `policies.yaml:1055`. A code search of `.cc` and `.h` files returned no C++ reader of the user-level policy (INFERRED: search coverage may miss files). Source: https://github.com/chromium/chromium/blob/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionManifestV2Availability.yaml

Names present in the current source, and what removal would involve:

| Name | Where | Role |
|---|---|---|
| `ShouldDisableLegacyExtensions()` | `extensions/browser/manifest_v2_handler.cc:111` | The gate. Hard-coded true. |
| `g_allow_mv2_for_testing`, `ScopedTestMV2Enabler` | same file `:35`, `:345`; `manifest_v2_handler.h:31` | Test override only. |
| `ManifestV2Handler`, `ManifestV2HandlerFactory` | `extensions/browser/manifest_v2_handler.{h,cc}` | KeyedService that disables and acknowledges affected extensions. |
| `manifest_v2_util::IsExtensionAffected` | `extensions/browser/manifest_v2_util.cc:15-45` | Affected = MV2 or lower, extension type, not component. |
| `mv2_deprecation_unsupported_ack_globally` | `manifest_v2_handler.cc:136` | Pref. |
| `ExtensionManifestV2Availability` | policy YAML (deprecated) | Enterprise override. No C++ reader found. |
| `allow-legacy-extension-manifests` | `switches.cc:15` | Manifest versions below 2. Not MV2. |
| `kManifestV2IsDeprecatedWarning` | `extension.cc` | Install warning for unpacked MV2. |

Minimum change. Make `ShouldDisableLegacyExtensions()` return false. Helium and Brave do exactly this (sections 2.1 and 2.3). Full removal would also touch the `ManifestV2Handler` service, its prefs, the affected-extension checks above, `chrome://extensions` strings (Helium's patch removes `is_affected_by_mv2_deprecation` from `extension_info_generator.cc`), the policy schema and its histogram enum, and the MV2 unit and browser tests. The one-function change is the smallest patch that works in the forks.

### Chromium main is still moving

INFERRED. Recent commits to `manifest_v2_handler.cc` on `main` include "Rename ManifestV2ExperimentManager to ManifestV2Handler" (2026-06-11), "De-class MV2DeprecationImpactChecker" (2026-07-08), and "Move out old mv2 deprecation prefs" (2026-07-11). The code is being cleaned up, not reverted. A future version could delete the gate and the MV2 code path together. Commit history from `gh api repos/chromium/chromium/commits?path=extensions/browser/manifest_v2_handler.cc`.

## 2. How forks keep uBO MV2 working

### 2.1 Helium

Helium is the reference for this project. Its repo is GPL-3.0 (`LICENSE`, repo metadata). Its README says Helium-unique code is GPL-3.0, and imported code keeps its own license (https://github.com/imputnet/helium/blob/main/README.md, "License").

VERIFIED mechanisms, all read from `imputnet/helium` (accessed 2026-10-23):

1. Global gate flip. `patches/ungoogled-chromium/extensions-manifestv2.patch`, listed in `patches/series:31`. It replaces the body of `ShouldDisableLegacyExtensions()` with `return false;`. It also removes the MV2 install warning in `extensions/common/extension.cc` and the `is_affected_by_mv2_deprecation` field in `extension_info_generator.cc`. Its `extensions_ui.cc` hunk changes a Safety Hub flag, not an MV2 notice. The file is byte-identical to ungoogled-chromium's copy at `patches/core/ungoogled-chromium/extensions-manifestv2.patch` (diff checked). https://github.com/imputnet/helium/blob/main/patches/ungoogled-chromium/extensions-manifestv2.patch
2. Built-in component. `patches/helium/core/ublock-install-as-component.patch` adds `ComponentLoader::AddUBlock()`, which loads `IDR_UBLOCK_MANIFEST_JSON` from `third_party/ublock`. It adds `helium::kUBlockOriginComponentId` ("blockjmkbacgjkknlgpkjjiijinjdanf") to the component allowlist (`component_extensions_allowlist/allowlist.cc`). The new header `components/helium_services/extension_ids.h` holds the Web Store ID ("cjpalhdlnbpafiamejdnhcphjbkeiagm") and the component ID. Commit that last touched it: `46c7ea4bda`, 2026-10-21 ("patches: refresh for 156.0.8078.47"). https://github.com/imputnet/helium/blob/46c7ea4bda/patches/helium/core/ublock-install-as-component.patch
3. Component exemptions. `patches/helium/core/ublock-reconfigure-defaults.patch` adds `Manifest::IsUBlockComponent()`. It excludes uBO from three privileges that Chromium gives components: the chrome:// resources and theme hosts (`chrome_content_browser_client.cc`), the CORS origin allowlist entry (`chrome_extensions_client.cc`), and the script-everywhere check in `PermissionsData::CanExecuteScriptEverywhere` (`permissions_data.cc`). The patch does not state a reason (INFERRED: Helium does not want a third-party component to hold Chromium's component privileges).
4. Vendored source. `patches/helium/core/ublock-setup-sources.patch` adds `third_party/ublock` with a `README.chromium`. It says: "Version: 1.66.4", "License: GPL-3.0", and "Patched out remote URLs from assets.json, patched in public key into manifest". The build script `generate_file_list.py` gzips the `assets/*.txt|*.dat` files into the bundle.
5. Remote-service rewiring. `ublock-helium-services.patch` points uBO's asset URL at Helium's own service (`/ubo/assets.json`). `ublock-dns-uncloaking.patch` adds a `dns.resolve()` extension API so uBO can do CNAME uncloaking (VERIFIED from the patch text; the API adds `extensions/common/api/dns.webidl`). `proxy-extension-downloads.patch` replaces the Web Store update URL with a placeholder (`https://update-url-to-be-replaced.qjz9zk`) that Helium's service handles.

Helium's uBO fork. `imputnet/uBlock` (GPL-3.0, repo metadata) has `platform/chromium/manifest.json` with `manifest_version: 2`, permissions including `webRequestBlocking`, `dns`, and `privacy`, and `version: 1.15.11.0` (VERIFIED, https://github.com/imputnet/uBlock/blob/main/platform/chromium/manifest.json). The `version` string does not match `README.chromium`'s 1.66.4, and the upstream CHANGELOG head reads `1.75.0`. The committed version string is probably a template that the release build stamps (INFERRED). Helium's `generate_file_list.py` expects a `manifest.json` at the root of `third_party/ublock`, so a build step must copy the platform manifest there (INFERRED).

Conclusion for Helium. Yes, Helium ships uBO MV2 (VERIFIED from the fork manifest and the patches). It does it with both mechanisms: the global flip from ungoogled makes Web Store and unpacked MV2 installs work, and the built-in component is exempt from the affected-extension check in any case.

### 2.2 Ardia (our reference fork, orientation only)

Read-only, from `/Users/tonibergholm/Developer/arctos/ardia`.

- `engine/patches/ungoogled-status.tsv` lists `core/ungoogled-chromium/extensions-manifestv2.patch` as `applied exact`.
- `docs/engine-build.md:222-226` says the successor to an uBOL build is "full uBlock Origin (MV2)", which "loads straight from the signed bundle (`Contents/Resources/Extensions/ublock`)". Bundled as a component.
- `docs/engine-build.md:30` lists `chromium-helium-ublock-off.patch`, which turns off Helium's own uBO component. Ardia runs its own uBO ID (`fajbdaomknpkicfjcpipdmcpoamdmilh`) so two engines do not both filter requests.
- `docs/engine-build.md:29` lists `chromium-component-extension-restricted-urls.patch`. This restricts a bundled component's script-injection reach to the normal Chrome checks.

So the approach is already proven on this Chromium line by two forks, one of them ours.

### 2.3 Brave

VERIFIED from `brave/brave-core` (accessed 2026-10-23):

- `chromium_src/extensions/browser/manifest_v2_handler.cc` defines `BRAVE_SHOULD_DISABLE_LEGACY_EXTENSIONS return false;`, then includes the Chromium file. Its comment says the override "is used to allow using MV2 extensions". https://github.com/brave/brave-core/blob/master/chromium_src/extensions/browser/manifest_v2_handler.cc
- `patches/extensions-browser-manifest_v2_handler.cc.patch` inserts that macro at the top of `ShouldDisableLegacyExtensions()`. Same function as Helium's patch, different mechanism.
- `browser/extensions/manifest_v2/brave_extensions_manifest_v2_installer.cc` installs a fixed set of MV2 extensions itself. `GetCrxDownloadUrl()` only accepts a CRX codebase when `brave::ShouldAddBraveServicesKeyHeader(url)` is true, so the CRX must come from Brave's own servers. The installer uses the update-manifest flow (`extension_urls::GetWebstoreUpdateUrl()`).
- `components/third_party/adblock/lists/ublock/README.chromium` lists uBO as GPL-3.0-or-later.
- Brave's public page says Brave "has committed to continue supporting some Manifest V2 extensions", naming AdGuard, NoScript, uBlock Origin, and uMatrix. It names no dates or conditions. (Fetched 2026-10-23 through a summarizer. https://brave.com/learn/extension-no-longer-supported/)

Brave's approach is the same as Helium's plus self-hosted CRX delivery.

### 2.4 ungoogled-chromium

VERIFIED. The patch `patches/core/ungoogled-chromium/extensions-manifestv2.patch` lives in `ungoogled-software/ungoogled-chromium` (license BSD-3-Clause, repo metadata). Helium and Ardia both carry the same file. The ungoogled patch set also includes `disable-webstore-urls.patch`, which Helium uses. It removes the Web Store launch and update URLs and the dedicated-process rule for the Web Store origin. https://github.com/imputnet/helium/blob/main/patches/ungoogled-chromium/disable-webstore-urls.patch

### 2.5 Vivaldi, Thorium, Edge

- Vivaldi: UNKNOWN. Secondary reports (piunikaweb.com, 2026-07-13; forum.vivaldi.net user posts) say MV2 extensions should keep working through the Vivaldi 8.1 release cycle, and that Vivaldi does not expect to keep them long term. Vivaldi's built-in blocker is said to continue. I found no official Vivaldi statement. The forum thread I fetched (forum.vivaldi.net/post/916511) contains only user comments.
- Thorium: UNKNOWN. The Thorium repo (`Alex313031/Thorium`, BSD-3-Clause) lists no MV2 or uBO patch in its tree (the only hits were Widevine and Flatpak patches). A secondary, undated note says Thorium force-enables MV2. Treat as unverified.
- Edge: not checked.

## 3. CEF and extensions

### 3.1 Runtime styles that still exist

VERIFIED. `include/internal/cef_types_runtime.h:64-74` defines `CEF_RUNTIME_STYLE_DEFAULT`, `CEF_RUNTIME_STYLE_CHROME`, and `CEF_RUNTIME_STYLE_ALLOY`. The header comment says Chrome style "provides the full Chrome UI and browser functionality" and Alloy style "provides less default browser functionality but adds additional client callbacks and support for windowless (off-screen) rendering". Windowless rendering always uses Alloy style. Source: https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h

The Alloy bootstrap is gone. CEF issue #3685 "alloy: Delete Alloy bootstrap (M128)" closed on 2024-07-01. Alloy-style windows still exist, but they run on the Chrome bootstrap. https://github.com/chromiumembedded/cef/issues/3685

### 3.2 The extension API is gone

VERIFIED.

- Commit `a461a89728` ("Delete Alloy bootstrap (fixes #3685)", 2024-07-01) removed `include/cef_extension.h`, `include/cef_extension_handler.h`, `include/capi/cef_extension_capi.h`, and `include/capi/cef_extension_handler_capi.h`, and modified `include/cef_request_context.h`. Source: https://github.com/chromiumembedded/cef/commit/a461a89728
- Commit `b5d84c254d` (2024-05-01) had already marked the extension API as deprecated in comments before the removal ("Extension API is documented as deprecated in comments but not compiled out with this arg").
- Issue #3450 comment by magreenblatt (2024-05-03): "The extension API as it currently exists (CefExtension, CefExtensionHandler, etc) will be removed in #3685." https://github.com/chromiumembedded/cef/issues/3450
- At master, `include/cef_request_context.h` contains no `LoadExtension` or `GetExtension` symbol (grep count 0, 2026-10-23 read). So `CefRequestContext::LoadExtension` and `CefExtensionHandler` do not exist in current CEF. Earlier CEF versions had them (INFERRED from the removal commit).

Issue #3877 (open since 2025-01-30) shows the workaround people use. A user wrote a `extensions.settings` preference through `CefRequestContext::SetPreference`, and the extension loaded only after a restart. magreenblatt suggested "listening for the pref change inside CEF and then executing any additional logic required" (2025-03-21). https://github.com/chromiumembedded/cef/issues/3877

### 3.3 chrome://extensions and the Chrome runtime

VERIFIED from CEF issue text.

- Issue #3450 body: "Extensions with the Chrome runtime can currently be installed and managed via chrome://extensions and various existing admin mechanisms."
- Issue #3859 (closed 2024-12-18), magreenblatt: "Extension functionality that works at the RequestContext level (like network intercepts, etc) should already work with Alloy style browsers. Extension functionality that requires Chrome UI (like toolbars, etc) will not be supported with Alloy style browsers. You can create a windowed Chrome style browser if you wish to load chrome://extensions." An Alloy-style browser refuses the navigation: "Navigation to chrome://extensions/ is blocked in Alloy-style browser."
- Issue #4011 (closed 2025-10-06), magreenblatt: "Alloy style only supports Browser/Tab extension APIs to the extent necessary to load the PDF viewer. There are no plans for general-purpose Tab API support with Alloy style."

So a CEF Chrome-style windowed browser can host the extension UI. Alloy-style browsers (including off-screen) cannot.

### 3.4 Web Store and de-Googled builds

- The Web Store is a component extension in Chromium (`ComponentLoader::AddWebStoreApp`, seen in Helium's patch context).
- UNKNOWN: whether CEF's Chrome runtime exposes the Web Store install UI by default. I found no CEF doc that covers it. I grepped `chrome_browser_extensions.patch` and `chrome_runtime.patch` for webstore strings and found no hits (VERIFIED for those two files only).
- Since 2026-08-31 the Web Store no longer lists MV2 extensions (section 1 timeline). MV2 uBO cannot be installed from the Web Store on any build, Googled or not. Only uBOL, the MV3 version, is still there. So for MV2 uBO the Web Store is not needed, and the only install path is one we bundle or preload.
- A de-Googled build does not need the Web Store for uBO if the extension ships in the bundle. Helium and Ardia both bundle uBO. Helium's Web Store proxy (`proxy-extension-downloads.patch`) appears aimed at Web Store installs (INFERRED from the patch name and its update-URL change).

### 3.5 `--load-extension` under CEF

VERIFIED from `chrome/browser/extensions/extension_service.cc:415-421` (main). CEF issue #4309 reports a repro that runs `cefclient --load-extension=<dir>` with an MV3 extension, and the extension's service worker runs (`ORIGIN_PROBE null`). That is a user report, not a run of ours, but it shows the switch works on current CEF for MV3 unpacked extensions (VERIFIED from the issue text). The `--load-extension` switch (`extensions/common/switches.cc:37`) is ignored only when `GOOGLE_CHROME_BRANDING` is set. Also ignored when the user has Enhanced Safe Browsing on, or when a policy blocks command-line extensions. Our CEF build is not Google Chrome branded (INFERRED), so the switch should work. It still loads through the MV2 gate: command-line extensions are not component locations, so an MV2 uBO loaded this way needs the same one-function patch.

### 3.6 Known CEF extension issues (open or recent)

- #4309 (open, 2026-10-12): an MV3 extension service worker's cross-origin `fetch()` is sent with `Origin: null` in CEF, while Chrome 148 sends no `Origin` header. Affects extensions that fetch cross-origin from a service worker. Whether uBOL does this is UNKNOWN.
- #4256 and #4255 (closed, 2026-08-26): `chrome.tabs.update()` issues for Alloy and off-screen browsers. One crashes the process.
- #3877 (open): pref changes are not applied until restart.
- #3450 (open since 2023): programmatic extension management is a wish, not a feature. The issue body asks for apps to "pre-install a bundled or downloaded extension".

## 4. uBO MV2 versus uBO Lite (MV3)

Sources: `gorhill/uBlock` wiki (cloned at `/tmp/research-03-ublock/wiki`) and the `uBlockOrigin/uBOL-home` wiki FAQ (`Frequently-asked-questions-(FAQ).md`), both read 2026-10-23.

What uBO MV2 has and uBOL does not:

- Dynamic filtering and dynamic URL filtering. The FAQ says these "can't be supported". Chromium 145 added `topDomains` and `excludedTopDomains`, which make part of it "theoretically partially possible". Dynamic filtering would need a `webRequest` listener that wakes the MV3 service worker.
- Per-site switches for remote fonts and scripting (only partly possible now).
- No-large-media-elements switch (DNR cannot read response headers).
- `strict1p` and `strict3p` (same-origin checks), entity-based `domain=`, `redirect-rule=`, regex `removeparam=`, exceptions for modifier options, `replace=` (response body), `ipaddress=`, `urlskip=`.
- CNAME uncloaking. "No DNR implementation supports this capability at the time of writing."
- Generic cosmetic filtering is off by default in uBOL. The user must raise the blocking mode to Complete.

What uBOL has that uBO MV2 lacks:

- Reliable filtering at browser launch and on navigation while its service worker is suspended. uBOL is entirely declarative, so it does not depend on the MV3 service worker waking up.
- A lighter service worker (the FAQ compares it with ABP 4.1 and gives task-manager captures).
- DNR limits: the FAQ cites a 330K global static-rule limit in Chromium. uBOL's default ruleset is about 17K rules.

The uBOL FAQ also says custom filters and external list subscriptions are now available (an "Edit, June 2026" note).

For a browser that wants uBO-level blocking, uBOL is not a drop-in replacement. The FAQ says so directly: "You will have to explicitly find a replacement to uBO according to what you expect from a content blocker."

## 5. Options with evidence

### Option A. Chromium extension system through CEF Chrome runtime, with MV2 patched back in

- Feasibility: high on paper. Helium, Brave, Ardia, and ungoogled-chromium all ship this exact flip on Chromium 155 and 156. The function is the same in 155, 156, and 158 main.
- Mechanism: (1) flip `ShouldDisableLegacyExtensions()`, (2) bundle uBO as a component extension (Helium's `AddUBlock` route), or load it unpacked (`--load-extension`) and accept the flip, (3) run Chrome-style windows so `chrome://extensions` and the toolbar exist.
- Maintenance cost: one function to track on every Chromium bump, plus the component patch, the allowlist, and the uBO build pipeline. The gate could also be removed in a future Chromium, which would break the flip (section 1).
- Not tested: CEF has not been run with any of this (UNKNOWN, section 6).

### Option B. Preload uBO through `CefRequestContext::LoadExtension`

- Not possible as written. The API is removed (section 3.2).
- Variants that remain: (B1) `--load-extension=<dir>` with an unpacked MV2 uBO. Needs the option A flip. (B2) `--load-extension` with MV3 uBOL. No MV2 flip needed (INFERRED). Gives uBOL's feature set, not uBO's. (B3) a preference write through `CefRequestContext::SetPreference` (the #3877 pattern). Known to need a restart, so not a fit for first run. (B4) Chromium's admin and pre-installed mechanisms, which the #3450 issue links: an external-extensions JSON file, or the `ExtensionInstallForcelist` or `ExtensionSettings` policies with a self-hosted `update_url` (INFERRED from the issue text and Chromium's documented paths; not tested). These need no Chromium patch for uBOL. MV2 uBO still needs the gate flip, because policy and external installs are not component locations (`manifest_v2_util.cc:37-40`).

### Option C. Our own network blocker in Rust, hooked into CEF

- Engine: `brave/adblock-rust` (MPL-2.0). The README lists network blocking, cosmetic filtering, resource replacements, hosts syntax, and uBO syntax extensions. https://github.com/brave/adblock-rust/blob/master/README.md
- API (VERIFIED in `src/engine.rs`): `Engine::check_network_request(&Request)` at line 254, `use_resources()` at line 331 (scriptlet and resource replacement), `url_cosmetic_resources(&str)` at line 381. The result type `UrlSpecificResources` (`src/cosmetic_filter_cache.rs:31`) has `hide_selectors`, `procedural_actions` (JSON strings the caller must run), `exceptions`, `injected_script` (scriptlet JS), and `generichide`.
- CEF hook (VERIFIED in `include/cef_resource_request_handler.h`): `CefRequestHandler::GetResourceRequestHandler()` (`cef_request_handler.h:141`) returns a `CefResourceRequestHandler`. Its `OnBeforeResourceLoad(browser, frame, request, callback)` at line 91 can return `RV_CONTINUE`, `RV_CANCEL`, or `RV_CONTINUE_ASYNC`. Changing the request URL counts as a redirect. The header notes that `browser` and `frame` may be NULL for service-worker and `CefURLRequest` requests, so the matcher must handle a missing frame. Evidence that the hook covers Chrome-style profiles, including extension traffic: the #4309 analysis says `ChromeContentBrowserClientCef::WillCreateURLLoaderFactory` puts `ProxyURLLoaderFactory` in front of every factory for CEF-created profiles, including extension service workers (VERIFIED from the issue text; I did not read that source file).
- Cosmetic and scriptlets: insert a stylesheet for `hide_selectors`. Run `injected_script` in the main world through `CefRenderProcessHandler::OnContextCreated` (`include/cef_render_process_handler.h:98`). The renderer then runs the scriptlets before page scripts. Procedural filters need a JS runtime that interprets the JSON actions (uBO's own code does that), so we would port or reuse that code.
- Gaps: no CNAME uncloaking at this layer. Resource URLs for uBO's own scriptlets come from the `resource-assembler` feature of adblock-rust. The timing and completeness of `OnBeforeResourceLoad` on Chrome-style windows are UNKNOWN and need a test. Existence is supported by the #4309 analysis.
- Brave's `components/third_party/adblock/lists/ublock` holds uBO's resource and scriptlet set that feeds Brave Shields, not the MV2 extension (VERIFIED from its `README.chromium`). Porting those scriptlets would still ship GPL-3.0 uBO code. That is fine for a GPL-3.0 app.
- Maintenance cost: we own filter updates, parsing bugs, scriptlet and procedural-filter code, and the list-license review. Lower Chromium coupling than option A, because it uses only the public CEF request API.

### Option D. Combination

- A and C together are possible: uBO MV2 from option A for power users, adblock-rust for the baseline, or uBOL (option B2) as a fallback if the MV2 flip breaks.
- Running two engines in one browser is a known problem. uBO's own FAQ says not to combine content blockers, and Ardia turns Helium's uBO off for the same reason. Pick one engine per profile.

## 6. Recommendation and key unknowns

Recommended path: option A, built the way Helium and Ardia build it. Bundle uBO MV2 (from `imputnet/uBlock` or `gorhill/uBlock`, GPL-3.0) as a component extension, apply the ungoogled `extensions-manifestv2.patch` (or the one-function equivalent Brave uses), and run Chrome-style windows. Keep option C as a spike, not as a parallel build. Its cost is an engine we own, and it is only worth building if option A fails.

Why: the same flip is already shipping on the Chromium line we target (156), it is one function, and it has the least new code. Option B as written is dead, because `LoadExtension` is gone from CEF. Option C works but gives us a new blocker to maintain.

Top risks:

1. Chromium deletes the MV2 code path. The gate is one function on `main` today (158), but the surrounding code is being refactored (section 1). We would need to notice on every Chromium bump, or ship a fork of the MV2 handler.
2. CEF has no public extension API, and its Chrome-style extension support is patch-level code. Issues #4309, #4255, #4256, and #3877 are open or recent. Each CEF upgrade may change extension behavior.
3. Updates. A bundled uBO and its filter lists need an update path that does not depend on the Web Store. Helium runs its own services (`/ubo/assets.json`, extension proxy). We would need to build that or use upstream URLs with CNAME-uncloaking and privacy effects reviewed.

Unknowns the prototype must settle (each is an experiment, not a reading task):

1. Under a CEF Chrome-style window built with a non-Google-branded Chromium, does a bundled MV2 uBO load as a component with `webRequestBlocking` active on the first launch, with no restart? Helium's `AddUBlock()` sits inside `if (!skip_session_components)` in `ComponentLoader::AddDefaultComponentExtensions` (Chromium main, `chrome/browser/extensions/component_loader.cc:719-729`). CEF's named patches and `chrome_browser_context.cc` have no `ComponentLoader` references, so CEF profiles probably reach that branch (INFERRED). If they do not, the component route needs one more patch, and that cost belongs in option A. Issue #3877 suggests the pref path needs a restart; component loading may differ.
2. Does `extensions-manifestv2.patch` plus the Helium component patch apply cleanly to CEF's Chromium 156 patch set? CEF has its own patch series (`patch/patches/`), so conflicts are likely in `chrome_browser_extensions*.patch`. This is a rebase test, not a reading task.
3. Does the uBO `dns.resolve()` CNAME-uncloak work in our build? It needs the Helium DNS patch.
4. Does `chrome://extensions` and toolbar pinning work in our window type, and can we hide the extension UI we do not want?
5. What are the timing and completeness of `OnBeforeResourceLoad` on Chrome-style windows? Existence is supported by the #4309 analysis (section 5C). Measure request coverage with service-worker and `CefURLRequest` traffic, to decide whether option C works as a fallback.
6. Does uBOL (MV3) load with `--load-extension` and block correctly? (Only if option B2 is kept as a fallback.)

Licensing (section 7 below) does not block option A or C.

## 7. Licensing

- uBlock Origin: GPL-3.0. `LICENSE.txt` in `gorhill/uBlock` is the GNU GPL v3 text (VERIFIED, https://github.com/gorhill/uBlock/blob/master/LICENSE.txt). The imputnet fork is GPL-3.0 per repo metadata. Brave's `README.chromium` for its uBO copy says GPL-3.0-or-later.
- adblock-rust: MPL-2.0 (VERIFIED, `LICENSE` at https://github.com/brave/adblock-rust/blob/master/LICENSE; repo metadata agrees). Section 3.3 of MPL-2.0 lets you distribute a Larger Work under a Secondary License such as GPL, provided the MPL-covered files remain under the MPL. The text reads: "this License permits You to additionally distribute such Covered Software under the terms of such Secondary License(s)" (VERIFIED in the file). The FSF license list lists MPL-2.0 as compatible with GPL, with the same condition (VERIFIED, https://www.gnu.org/licenses/license-list.html, MPL-2.0 entry). The MPL can be opted out by a notice saying "Incompatible With Secondary Licenses". I did not find such a notice in the adblock-rust README; I did not check every source file header (INFERRED).
- Chromium: BSD-style license (header of `extensions/browser/manifest_v2_handler.cc`, VERIFIED). CEF: BSD-style (header of `include/cef_request_context.h`, VERIFIED). ungoogled-chromium: BSD-3-Clause (repo metadata, VERIFIED).
- Filter lists: uBO's default lists are GPL-3 or CC BY-SA. Some optional lists are non-commercial, public domain, or undetermined. Source: https://github.com/gorhill/uBlock/wiki/Filter-list-licenses. Bundle only the default lists under their licenses, or fetch the rest at runtime.
- Combined GPL-3.0 app: uBO (GPL-3.0) combines without a license conflict. MPL-2.0 code combines under GPL-3.0 with notices kept. Keep the license files and source offers for uBO and adblock-rust in the distribution.

Compatibility conclusion: bundling uBO and adblock-rust in a GPL-3.0 app is compatible. This is a reading of the licenses, not legal advice.

## 8. What I could not verify

- Chromium 156 as a whole. I read the MV2 handler, `manifest_v2_util.cc`, `manifest.h`, `extension.cc`, `switches.cc`, and `extension_service.cc` at tag 156.0.8078.47 through googlesource. The policy schema and the `ExtensionManifestV2Availability` C++ readers were checked on `main` (158) only, with a code search that may miss files.
- Whether CEF master at 156 includes the Web Store flow, and how it behaves. I did not grep the CEF patch contents.
- Any run of CEF, Chromium, or uBO. No builds were made. Everything about runtime behavior is UNKNOWN until the prototype runs. The #4309 repro and the #3859 and #3450 statements are user or maintainer reports, not our runs.
- Whether CEF's Chrome-style profiles reach `AddDefaultComponentExtensions`'s session branch (INFERRED from the absence of `ComponentLoader` references in five files, not a full patch-set read).
- Whether the CEF policy readers were checked on 156. The policy schema and the C++ reader search were checked on `main` (158) only.
- Vivaldi, Thorium, and Edge official MV2 positions (secondary sources only).
- Whether uBOL (MV3) under CEF handles `Origin: null` (issue #4309) for its own requests.
- Whether the GitHub code search results for "LoadExtension" and "ManifestV2" in `chromiumembedded/cef` were complete. Some code search calls hit the rate limit.

## Sources list

- Chrome MV2 timeline: https://developer.chrome.com/docs/extensions/develop/migrate/mv2-deprecation-timeline
- Chromium MV2 handler: https://chromium.googlesource.com/chromium/src/+/refs/tags/156.0.8078.47/extensions/browser/manifest_v2_handler.cc and https://github.com/chromium/chromium/blob/main/extensions/browser/manifest_v2_handler.cc
- Chromium MV2 util: https://github.com/chromium/chromium/blob/main/extensions/browser/manifest_v2_util.cc
- Chromium extension switches: https://github.com/chromium/chromium/blob/main/extensions/common/switches.cc
- Chromium extension service: https://github.com/chromium/chromium/blob/main/chrome/browser/extensions/extension_service.cc
- Chromium MV2 policy: https://github.com/chromium/chromium/blob/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionManifestV2Availability.yaml
- Helium repo: https://github.com/imputnet/helium (README, `chromium_version.txt`, `patches/series`, `patches/helium/core/ublock-*.patch`, `patches/ungoogled-chromium/extensions-manifestv2.patch`)
- Helium uBO fork: https://github.com/imputnet/uBlock (`platform/chromium/manifest.json`)
- uBO upstream: https://github.com/gorhill/uBlock (`platform/chromium/manifest.json`, `CHANGELOG.md`, `REMOVED.md`, `LICENSE.txt`, wiki)
- uBO wiki page on the Chrome notice: https://github.com/gorhill/uBlock/wiki/About-Google-Chrome's-%22This-extension-may-soon-no-longer-be-supported%22
- uBOL FAQ: https://github.com/uBlockOrigin/uBOL-home/wiki/Frequently-asked-questions-(FAQ)
- Brave: https://github.com/brave/brave-core/blob/master/chromium_src/extensions/browser/manifest_v2_handler.cc, `browser/extensions/manifest_v2/brave_extensions_manifest_v2_installer.cc`, and https://brave.com/learn/extension-no-longer-supported/
- ungoogled-chromium: https://github.com/ungoogled-software/ungoogled-chromium (`patches/core/ungoogled-chromium/extensions-manifestv2.patch`)
- Ardia (read-only): `/Users/tonibergholm/Developer/arctos/ardia/docs/engine-build.md`, `engine/patches/ungoogled-status.tsv`
- CEF runtime types: https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h
- CEF removal commit: https://github.com/chromiumembedded/cef/commit/a461a89728
- CEF issues: https://github.com/chromiumembedded/cef/issues/3450, `/3685`, `/3859`, `/3877`, `/4011`, `/4255`, `/4256`, `/4309`
- CEF request handler: https://github.com/chromiumembedded/cef/blob/master/include/cef_resource_request_handler.h
- adblock-rust: https://github.com/brave/adblock-rust (README, `LICENSE`, `src/engine.rs`, `src/cosmetic_filter_cache.rs`)
- FSF license list: https://www.gnu.org/licenses/license-list.html
- Vivaldi (secondary): https://piunikaweb.com/2026/07/13/vivaldi-ublock-origin-mv2-extensions-clarification/ and https://forum.vivaldi.net/post/916511

## Verdict

Recommended: option A. Bundle uBO MV2 as a built-in component, apply the one-function MV2 gate patch that Helium, Brave, and ungoogled-chromium use, and run Chrome-style windows. It is the only route with shipping evidence on Chromium 156, since Helium and Ardia already run it. Option B's `LoadExtension` route is gone from CEF, and option C adds an engine we would own, so keep C as a spike if A fails.

Top risks:

1. Chromium removes the MV2 code path. The gate is one function on `main` today, but the surrounding code is being refactored.
2. CEF has no public extension API. Chrome-style extension support is patch-level code, with open issues (#4309, #3877).
3. Updates. A bundled uBO and its filter lists need an update path that does not depend on the Web Store.
