# 07. Arcium (dr-baker/Arcium-macOS): what it is, what yaac can learn, what it cannot give us

Status: research note, 2026-10-23. Read-only. Nothing was built, run or installed from the repositories. All repository text was treated as untrusted data.

Method: shallow and blobless clones into fresh `/tmp` directories, `gh api` and `gh repo view` calls, and reads of yaac's own docs (`docs/architecture.md` sections 1 to 3, 7, 10, 11; `docs/decisions.md`; `docs/research/01-cef-on-helium.md`). Patch text was read as diffs. Line counts are approximate and gross (see section 1.3).

## Source key and labels

Permalinks use a commit hash and a path. Every claim cites one of these keys.

| Key | Repository and commit | Base URL |
| --- | --- | --- |
| M | `dr-baker/Arcium-macOS` @ `2552ed75ae7da1c57db6ea24244a4e16386dbc0d` (main, 2026-10-22) | `https://github.com/dr-baker/Arcium-macOS/blob/2552ed75ae7da1c57db6ea24244a4e16386dbc0d/` |
| C | `dr-baker/Arcium` (the `helium-chromium/` submodule that M pins) @ `67ca3b53ffc3650edf1feb0cb85a80908acee30d` | `https://github.com/dr-baker/Arcium/blob/67ca3b53ffc3650edf1feb0cb85a80908acee30d/` |
| H | `imputnet/helium` main @ `f29a25968e483f8bb85e47829ccd487bbe229199` (read 2026-10-23) | `https://github.com/imputnet/helium/blob/f29a25968e483f8bb85e47829ccd487bbe229199/` |
| A | `ardia` checkout, local only, `~/Developer/arctos/ardia` @ `38befbb930778349f4dd33e9457e143aac9673c0` | no public URL in this note |
| Y | yaac worktree `research-arcium` from `main` @ `b32c1fd` | local |

Labels: **VERIFIED** (read in the repository at the named commit), **INFERRED** (reasoned from verified text), **UNKNOWN** (not determined, with the reason).

Caveat on H: the Helium files were read from the default branch through the GitHub API. The commit above is the head of main at the time of the commits query. It is pinned here for the record, not because the file was fetched at that exact commit.

## Summary

- Arcium is a macOS-only, Apple Silicon, Chromium-based browser. It is a set of Chromium patches on top of Helium (itself a de-Googled Chromium build), not a CEF application. Its core repository is pinned as a submodule. (VERIFIED, M `.gitmodules`; C `chromium_version.txt`.)
- The product is Chromium 151.0.7922.71. Upstream Helium's main is on Chromium 156.0.8078.47. Arcium has not pulled an upstream Helium update since 2026-07-30 (VERIFIED, M and C git history and H `chromium_version.txt`). It is five milestones behind.
- Almost all of the work is in Chromium browser UI (`chrome/browser/ui/...`) and is written as Quilt patches. Reusing it inside yaac's CEF and web-UI design would need a rewrite, not a copy.
- Provenance has open questions. The vendored native Swift Settings package (SettingsUI) has an origin and licence that I could not establish. The Arc importer reads Arc's private `StorableSidebar.json` schema. Most Arcium headers read "GPL-3.0 license" with no "or later" (C `patches/personal`, 116 added lines), which reads as GPL-3.0 only.
- Governance is a single-maintainer project. Since 2026-08-20 one author has made 415 of 416 commits in M and 308 of 310 in C. There are no issues ever filed in either repository (VERIFIED, git history and `gh api`).
- The project's AI policy is permissive: "AI-assisted development is welcome" (C `AGENTS.md`). Helium's policy is the opposite for contributions to Helium (H `AGENTS.md`). That matters for option (b) in section 8.
- The product's distinctive behaviour (Pins that keep a home page, Favorites per profile, Peek, Little Arc, the command bar scopes, the recent-tab switcher, and the sidebar importer) is worth studying as behaviour notes. The code is not a reuse candidate for yaac.

Licence and provenance risk, in one line: the code is GPL-3.0 and cannot be copied without changing yaac's "or later" licence, and the Swift UI and importer have provenance questions that block reuse until checked.

---

## 1. Architecture and build

### 1.1 Layout

- M is the product repository. It owns the build scripts (`build.sh`, `dev.sh`), the `he` development tooling (`devutils/he/`), packaging, signing, resources, and the macOS-only patches under `patches/`. (VERIFIED, M `AGENTS.md`, `docs/building.md`.)
- C is the shared "Arcium core". It owns `patches/` (ordered Quilt patches), `resources/`, `devutils/` (lint and validators). The README says it is "the shared Arcium browser product". (VERIFIED, C `README.md`, `AGENTS.md`.)
- Patch order is upstream first: `upstream-fixes`, `ungoogled-chromium`, `helium`, then `personal` (Arcium). (VERIFIED, C `patches/series`, read in order.) This matches the Helium-first order that yaac's spike E1 found works (Y `docs/decisions.md`, "Patch order reversed").
- M's own series has 47 entries: 28 `personal`, 14 `helium/macos`, 3 `ungoogled-chromium/macos`, 1 `upstream-fixes`, 1 `rebel/macos` (Sparkle). (VERIFIED, M `patches/series`.)
- `helium-chromium/` in M points to `dr-baker/Arcium.git`, not to imputnet's Helium repository. The core therefore carries the Helium patches itself. (VERIFIED, M `.gitmodules`; C `patches/helium/`.)

### 1.2 Where the features live

| Feature area | Where | Evidence |
| --- | --- | --- |
| Spaces, Pins, Favorites, folders, split pins | `chrome/browser/ui/helium/`, `chrome/browser/ui/tabs/`, `components/tabs/` | C `patches/personal/arcium-spaces.patch`, `arcium-pins.patch`, `arcium-surface-groups...` (new files, see 2.2) |
| Command bar (Command-T), scopes | `chrome/browser/ui/omnibox/arcium_command_*`, `components/omnibox/browser/arcium_command_search.cc` | C `patches/personal/arcium-omnibox.patch`, `arcium-omnibox-panel.patch` |
| Peek and Little Arc | `chrome/browser/ui/helium/helium_little_arc.cc`, a separate WebContents for Peek | C `patches/personal/arcium-little-arc.patch`; Peek evidence notes in M `docs/feature-backlog.md` |
| Recent-tab switcher (MRU) | `chrome/browser/ui/views/frame/mru_tab_*` incl. a macOS `.mm` visualizer | C `patches/personal/arcium-mru-tab-switcher.patch`, `arcium-feature-performance.patch` |
| Glass material | `chrome/browser/ui/views/helium/glass_surface*.cc/.mm`, `cc/paint/glassmorphic_paint_filter.h` | C `patches/personal/arcium-glass-system.patch` |
| Native Settings | Swift (SwiftUI) hosted from Objective-C++ and an AppKit window in Chromium | C `patches/personal/arcium-native-settings-macos.patch`; M `patches/personal/arcium-settingsui-*.patch` |
| Arc import | `chrome/browser/importer/arcium_arc_import.cc`, `chrome/utility/importer/arc_sidebar_snapshot.cc` | C `patches/personal/arcium-arc-import.patch` (section 4) |
| Packaging, installer, Sparkle | `devutils/arcium/`, `devutils/preview-installer/main.swift`, `patches/rebel/macos/sparkle-integration.patch` | M tree |
| Glass prototype (Metal, Swift) | `devutils/glass-lab/` (a standalone Swift package with its own README and `prepare.py`) | M `devutils/glass-lab/`. Mentioned by M's SettingsUI patch and `docs/native-settings-bindings.md`. Not referenced by `patches/series`, `build.sh` or `dev.sh` (VERIFIED by grep). Not shipped: INFERRED. |

The "Metal" question: the product's glass is a `cc` paint filter and an Objective-C++ layer. The Metal shader in the tree is in the `glass-lab` prototype. Whether the shipped product uses Metal shaders directly is UNKNOWN from the text I read.

### 1.3 Size of the delta against upstream Helium

These numbers are approximate. Patch text is gross: later patches rewrite earlier ones, so added and removed lines overstate the net change. Blank added lines are excluded by the counting pattern.

| Set (C unless noted) | Patch files | Distinct path strings (upper bound) | Gross added / removed lines (approx.) |
| --- | --- | --- | --- |
| `patches/personal` (C) | 157 | 787 | ~75,800 / ~25,300 |
| `patches/helium` (C, upstream Helium) | 215 | 809 | ~16,900 / ~5,700 |
| `patches/ungoogled-chromium` (C) | 71 | 702 entries (incl. overlaps) | ~1,350 / ~7,700 |
| all of C (`patches/`, 9 vendor sets) | 462 | 2,117 (upper bound) | not summed |
| M `patches/personal` | 28 | part of M's 340 distinct paths | ~36,200 / ~15,600 |

Distinct-path counts are upper bounds. The patches mix `+++ src/...`, `+++ b/...` and bare `+++ chrome/...` prefixes, and `/dev/null` entries are counted, so one file can be counted twice. Line counts come from `^+` and `^-` lines at the commits named above and are approximate.

New files: 255 new files in the `personal` and `helium` sets, by extension: 114 `.cc`, 100 `.h`, 6 `.mm`, 5 `.ts`, 5 `.gn`, 4 `.html`, 2 `.mojom`, plus small numbers of other types (VERIFIED, `--- /dev/null` headers in those patches).

Tracked source outside patches (M, VERIFIED with `wc` at the commit): 11 Swift files (1,644 lines, all in `devutils/`), 49 Python files (10,771 lines), 27 shell scripts (1,928 lines). The product Swift is not in M's tree. It enters through patches, see 2.2.

### 1.4 Chromium and Helium versions, tracking, cadence, and bump cost

- Arcium: Chromium 151.0.7922.71 (C `chromium_version.txt`; M `docs/releases/0.2.0-preview.1.md`). Helium base in the preview manifest: 0.15.1.1 (M `resources/preview/oct17-source.json`).
- Upstream Helium main: Chromium 156.0.8078.47 (H `chromium_version.txt` at `f29a2596`, read 2026-10-23). Arcium is five milestones behind.
- Tracking history (VERIFIED, `git log` of M, blobless clone): 86 commits named `update: helium X` (weekly in July 2026, last `update: helium 0.15.1.1 (#306)` on 2026-07-30). A `helium/sparkle: fix build targets for chromium 151 (#308)` commit followed on 2026-07-31. A "private Arcium Chromium baseline" commit is dated 2026-08-20. No upstream update commit exists after 2026-07-30.
- Bump cost, from the documented procedure (M `docs/building.md`, "Updating for a new Chromium release"): create a new build directory, run `he presetup`, `he merge`, `he push`, then repair each rejected patch by hand with `quilt push -f` and `quilt refresh`, repeat until the series applies with zero fuzz, then configure, build, run, and run validators. The number of rejections is the cost driver. Arcium's strict-push rule (M `docs/development-changes.md`, "Strict push") means fuzzed hunks stop the build. I could not measure hours or rejection counts for the 151 to 156 jump; that is UNKNOWN.
- Cadence since the fork: 264 commits to M in October 2026 (to the 23rd), 196 to C. Almost all since 2026-08-20 are by one author (section 7).

### 1.5 Native settings UI

- The settings window is an AppKit `NSWindowController` per profile, with an `NSHostingController` child that renders SwiftUI. Chromium's settings coordinator owns the `ArciumSettingsModel`. SwiftUI receives snapshots and sends edits back through an Objective-C delegate. Preferences stay in Chromium. (VERIFIED, M `docs/settingsui-integration.md`, "Ownership".)
- The SwiftUI code is a vendored subset of a package named SettingsUI, pinned to canonical revision `b8f1fc81b91a48be3fdc4ea6f31e63e14ca62475`. Arcium uses three local modules and 81 Swift source files. The package has no external dependencies after the ColorTokensKit change (M `docs/settingsui-integration.md`, "Dependency provenance"). The vendored copy is in the patches (C/M `patches/personal/arcium-settingsui-*.patch`).
- Minimum macOS is raised to 26.0 in `build/config/mac/mac_sdk.gni` by patch (M `docs/settingsui-integration.md`, first section).
- `docs/native-settings-bindings.md` (40 KB) is a per-control binding map; I read its headings only. The scenario contracts in `devutils/scenarios/native-settings.json` define the test steps.
- For yaac: this path is native AppKit and SwiftUI on macOS. yaac's chrome is web UI in a CEF view (Y `docs/architecture.md` sections 1 and 2). The native settings approach does not transfer.

### 1.6 Build requirements, CI, tests

- Hardware and disk: Apple Silicon for the shipped preview (M `docs/preview-release.md`). About 100 GB free for the build directory (M `docs/building.md`, "First day"). `he setup` requires 100 GiB on the build volume (M `docs/development-changes.md`). The first compile "takes hours" (M `docs/building.md`). RAM and CPU requirements: UNKNOWN.
- Software: macOS 26 or later, Xcode 26 with Swift 6.2 or newer, Homebrew, Python 3.13, Metal toolchain, ninja, wget, quilt, coreutils (M `docs/building.md`, "Software requirements" and "Build dependencies").
- Optional: a NativeLink remote-build endpoint (M `docs/building.md`, "Remote builds"). A Developer ID certificate is needed for `he arcium` (install as the daily browser) (M `docs/building.md`, "Install as Arcium").
- CI (VERIFIED from `.github/workflows` in M and C): `lint.yml` runs `devutils/lint.py` from C on pushes and pull requests (patch series check, Ubuntu). `sanity.yml` runs Python `unittest` over `devutils/tests`. `build.yml` is `workflow_dispatch` only. The reusable build workflows (`build-init.yml`, `build-step.yml`, `building.yml`) are `workflow_call` only. So no pull-request job compiles Chromium or runs the browser tests (INFERRED from the triggers; I did not read every step).
- Tests that matter most are in the Chromium tree: unit tests and `browser_tests` compiled inside the build. They need a second build directory with full source (M `docs/building.md`, "Full source and browser tests"; M `docs/tab-invariants.md`). The tab-invariant runner (M `devutils/tab_invariants.py`) is the most instructive piece (section 3.4).

---

## 2. Provenance and licensing

### 2.1 Licence files

- `LICENSE` in M and C is the GNU GPL v3 text (VERIFIED, both repositories, first lines). `LICENSE.ungoogled_chromium` is BSD-3-Clause, "Copyright (c) 2015-2026, The ungoogled-chromium Authors" (VERIFIED, C).
- The README states the policy: original Arcium additions are GPL-3.0, and imported code keeps its licence (VERIFIED, M `README.md`, "Source and development"; C `README.md`, "License"). The "GPL-3.0" statement does not say "or later" (VERIFIED, C `README.md`).
- M's GitHub licence field is GPL-3.0 (VERIFIED, `gh repo view`).

### 2.2 Headers of new files

Counted from added (`+`) header lines across C `patches/personal` (this includes edits to existing files, not only new files):

| Header | Count (C personal) | What it means |
| --- | --- | --- |
| `Copyright YEAR The Helium Authors` + "GPL-3.0 license that can be found in the LICENSE file" | 124 | Inherited Helium convention. The named copyright holder is Helium's authors, not Arcium's. |
| `Copyright YEAR The Chromium Authors` | 25 | Chromium's header, on code patched into Chromium files. |
| `Copyright YEAR The Arcium Authors` | 7 | The only header that names Arcium. |
| Phosphor Icons MIT notice | 1 | Third-party licence text, shipped with the vendored icons. |

- The GPL header wording (`GPL-3.0 license`) is not qualified by "or later" anywhere in the core's added lines. In C `patches/personal`, 116 added lines contain `GPL-3.0 license`, and none says "GPL-3.0-or-later" or "or later" as a licence. The one line matching "or later" is a code comment ("for later execution"). (VERIFIED, C at `67ca3b53`.) Read literally, that makes those files GPL-3.0-only.
- yaac's own decisions are inconsistent on this point: `docs/decisions.md` says "GPL-3.0", `docs/architecture.md` section 1 says "GPL-3.0-or-later" (Y). Reusing a GPL-3.0-only file in a combined work would force that combined work to GPL-3.0-only, unless we are the sole copyright holder of the change and can relicense it. This needs a decision by the owner (section 9).
- `devutils/glass-lab/` Swift and Metal files have no licence header (VERIFIED: the first lines are `import AppKit`, `import CoreImage`). They rely on the repository-wide LICENSE.

### 2.3 Third-party code and assets

| Item | Where | What I found | Status |
| --- | --- | --- | --- |
| Phosphor icons | C `patches/personal/arcium-phosphor-icons.patch` (`third_party/phosphor-icons/LICENSE`, `README.chromium`) | MIT License text is added with the vendored icons. Icon data is generated by `devutils/generate_phosphor_icons.py` (GPL header). | VERIFIED MIT. Usable with attribution. |
| Space emoji catalogue | C `devutils/generate_space_emoji_data.py`, `patches/personal/arcium-space-icon-popover.patch` | Generated from Chromium's `third_party/cldr` names and keywords (the generator's docstring). | INFERRED: CLDR is under the Unicode licence. Not verified in these repos. |
| ColorTokensKit (Swift, by Siddhant Mehta, MIT) | M `patches/personal/arcium-settingsui-production.patch` (MIT-LICENSE.txt added), `arcium-settingsui-modules-macos.patch` (deleted) | MIT notice retained during the production patch. The modules patch deletes the package's files, including the LAB colour conversion files, and no later patch re-adds them (VERIFIED across `patches/series` order). Final tree: ColorTokensKit is no longer a dependency or app resource (M `docs/settingsui-integration.md`). | Final tree: not present. History: MIT. |
| ColorSpaces-derived colour files (`LABColor`, `LCHColor`, `RGBColor`, `XYZColor`) | M `patches/personal/arcium-native-settings-macos.patch` (adds the "Original repo: github.com/timrwood/ColorSpaces" header, 4 times), `arcium-settingsui-production.patch` (removes it and re-adds it, 4 and 4), `arcium-settingsui-modules-macos.patch` (removes it, 4 times) | The attribution comes and goes across the series. The modules patch deletes the ColorTokensKit files that carried it (VERIFIED, `+++ /dev/null` at M `arcium-settingsui-modules-macos.patch` lines ~18636 and ~18681). | Final tree: the files are gone. Licence of ColorSpaces itself: UNKNOWN (no LICENSE was read). |
| SettingsUI (Swift package, 81 files retained) | M `patches/personal/arcium-native-settings-macos.patch` (`VENDOR_PROVENANCE.md`, earlier text: "local `SettingsUI` checkout, revision `164b8920...`" and "SettingsUI has no tracked LICENSE file at the source revision"), `arcium-settingsui-production.patch` (line 4788: rewrites that text to "committed main revision `1667a11b...`"), `arcium-settingsui-modules-macos.patch` (final text names only "Canonical SettingsUI revision: `b8f1fc81...`") | The final text names a commit but no repository URL. The earlier text describes a local checkout and states that there is no LICENSE. The final text is silent on licence. | Searched on 2026-10-23: a GitHub commit-hash search for `b8f1fc81...` returned no matches, and a repository-name search for SettingsUI under `dr-baker` returned none. Public repositories named SettingsUI under other owners exist, but none is linked to Arcium in the repositories I read, and I did not compare them. **UNKNOWN origin and UNKNOWN licence.** Reuse of this code is blocked until the owner states both in writing. |
| Separate repository `dr-baker/mru-tab-switcher` | GitHub, not read | Created 2026-07-23; described as "a fast, privacy-conscious MRU tab switcher for Chromium"; GitHub API reports no detected licence | **UNKNOWN** relation to Arcium's MRU patch. Read its licence and history before reusing anything from either project. |
| Glass Metal shader | M `devutils/glass-lab/Sources/ArciumGlassLab/Resources/Glass.metal` | No header. Prototype tool. | Provenance UNKNOWN. |
| App icon | M `resources/assets/AppIcon.icon/Assets/logo.svg`, `resources/assets/Assets.car` (compiled) | The SVG is a simple geometric path with no attribution. `Assets.car` is a binary compiled asset catalogue. | Logo origin UNKNOWN. I did not compare it visually with any Arc asset. `Assets.car` contents: UNKNOWN. |
| Screenshots | M `docs/assets/*.png` (7 files) | Show the Arcium UI. | I did not view them for Arc marks. UNKNOWN. |
| Upstream Helium-derived resources, domain lists, pruning lists | C `domain_substitution.list`, `pruning.list`, `patches/helium/` | Inherited from the Helium lineage. | Licence follows Helium (GPL-3.0). |

### 2.4 Arc-derived material

- No Arc assets, binaries or strings are in these repositories. I searched added lines in the patches for "arc" (175 matches in `personal`, nearly all in identifiers and the importer) and for "Browser Company" (no matches) (VERIFIED, grep at C).
- Identifiers that mirror Arc's data: the `arc:` URI scheme as an internal source-ID namespace (for example `arc:<profile>:favorites-space` in C `patches/personal/arcium-arc-import.patch`), an "Arc Favorites" fallback Space name (C `patches/personal/arcium-arc-import.patch`), and the importer class names `arc_sidebar_importer` and `arc_sidebar_snapshot` (C `patches/helium/core/add-arc-importer.patch`, `patches/personal/arcium-arc-import.patch`).
- Feature names that Arcium describes as Arc-inspired: "Little Arc", "Peek", "Spaces", "Pins", "Favorites", "Shelf" (as a planned "Shelf-inspired alternative to auto-archiving", M `README.md`, "What's next"). Arcium's own README calls the product "Arc-inspired" (M `README.md`, first paragraph). Whether each name is an Arc product name is INFERRED from that wording, not verified. "Boost" is not in Arcium but is in yaac's own section 7. Whether these names are protected in trademark terms is a question for the owner, not for this note.
- Method disclosure: M `docs/import-from-arc.md` and `docs/arc-daily-driver-migration.md` describe Arc's sidebar data as "the captured `StorableSidebar.json`", "Arc's saved sidebar". They do not say how the schema was learned. The importer fixtures are synthetic (`example.test` domains, invented profile names, C `patches/personal/arcium-arc-import.patch`; ardia's `packages/import/fixtures/generate.py` states "Everything here is invented" (A)).
- Helium upstream already had an Arc importer that reads the same file (C `patches/helium/core/add-arc-importer.patch`). So knowledge of the on-disk format predates Arcium and comes from Helium (INFERRED from file history and naming). I found no evidence of reverse engineering of Arc's binaries, of decompilation, or of copied Arc code. Evidence of how the format was learned: UNKNOWN.
- The yaac brief (Y `docs/architecture.md` section 7) lists `parentID`, `createdAt`, `isUnread`, `originatingDevice` as Arc's fields. Neither Arcium's parser (C `patches/personal/arcium-arc-import.patch`, `arc_sidebar_snapshot.cc`) nor ardia's parser (A `packages/import/ios/Core/ArcSidebar.swift`) reads those keys (VERIFIED by search). Section 7's field list goes beyond what either importer uses. This does not show whether the fields exist in Arc.
- Distribution: the preview installer bundle identifier is `net.imput.arcium.installer` (M `resources/preview/Installer-Info.plist`). That namespace belongs to Helium's publisher. yaac should not reuse it.

### 2.5 Build provenance of the preview

- The 0.2.0-preview.1 binary was not built from source in a clean environment. It is "a preserved October 17 installed app", re-signed, and the original `args.gn` "was not preserved". The reconstructed GN arguments in `resources/preview/reconstructed-browser-args.gn` are documented as "not a fresh reproducible build" and "binary byte reproduction is not claimed" (VERIFIED, M `resources/preview/oct17-source.json`, `reconstructed-browser-args.gn`). The release notes say the same (M `docs/preview-release.md`, "Source and verification").

### 2.6 Risk per area for yaac reuse under GPL-3.0-or-later

See section 8.3 for the full table.

---

## 3. Features and behaviour (behaviour notes only, no code)

Each point cites the M docs or C patch that describes it. I did not run the product.

### 3.1 Spaces, Pins, Favorites

- Favorites are local to a browser profile and shared across its Spaces. Pins and folders belong to one Space. Spaces inside one profile share cookies and sign-ins. Separate accounts need separate profiles (M `README.md`, "Pins, Favorites, and Spaces"; M `docs/arc-quickstart.md`). Same rule as yaac's `Space` and `Favorites` model (Y `docs/architecture.md` section 7). VERIFIED for both.
- A Pin remembers its home page. Following links in the live tab does not change it. Closing the live tab leaves the Pin dormant, and opening it starts at the home page. There is a return-home control (M `docs/arc-quickstart.md`, "Keep useful pages"). yaac's `suspended` state (Y section 7) keeps url, title and favicon but does not define a home page separate from the current URL. This is the most useful behaviour to adopt, and it needs a separate `home_url` field on pinned nodes. (INFERRED: yaac's schema has `url` only.)
- A split in a Pin is one durable item. Pane identity is the ordered slot, not the URL, so two panes can show the same page (C `patches/personal/arcium-pins.patch`, comment on `PersistentSplit` in `helium_persistent_item_model.h`). yaac's split node (Y section 7: two to four `tab` children) allows the same, but the note should say identity is by position. Arcium's README lists "Pinned split views" as unfinished (M `README.md`, "What's next").
- Invariants: M `docs/tab-invariants.md` lists the rules the tests check. Model tier: unique live ownership, forward and reverse association agreement, no retired associations, rekey preserves identity, rejected tree moves are atomic, serialization preserves tree state. Service tier: Pin, Favorite, removal and selection sequences preserve tab count, WebContents, URL, selection and collection membership. Browser tier: visible rows agree with the model. The sequences are seeded and fixed. yaac's section 7 has tree-shape invariants and a store check at load. Arcium's seeded sequence tests are a stronger testing pattern to adopt.

### 3.2 Peek

- Peek opens an eligible link as a separate WebContents under the same browser profile, keeps the source page behind it, and blocks input to the source. Promote the preview to a tab or sidebar item, or close it (M `README.md`, "Peek and Little Arc"; C `patches/personal/arcium-peek.patch`, per the backlog's evidence note). Peek is on by default in new profiles (M `resources/preview/initial-preferences.json`, `helium.browser.peek_enabled = true`).
- The backlog has a deferred design for sign-in redirects inside Peek. It tries three approaches in order, and it insists that the original tab keep its deep link, query, hash, scroll, unsent draft and focus. It also says cookies alone do not prove a flow works (M `docs/feature-backlog.md`, "Open sign-in redirects in Peek"). This is the most careful piece of product thinking in the repository. Worth adopting as a requirement list, not as code.

### 3.3 Little Arc

- Opens one eligible HTTP or HTTPS link from another app in a compact window. Several URLs, other schemes, and an empty Little Arc search window (shortcut) are not supported (M `docs/arc-quickstart.md`, "Shortcuts and optional behavior"; M `README.md`). Enabled by default in new profiles (M `initial-preferences.json`, `helium.little_arc.external_links = true`).

### 3.4 Command bar and scopes

- Command-T opens the bar. Scopes are Omni, Web, Saved, YouTube, ChatGPT. Choosing YouTube or ChatGPT builds a search URL for that site (C `patches/personal/arcium-omnibox.patch`: `youtube.com/results?search_query=`, `chatgpt.com/?q=`; also an `x.com/search` template in the same patch). The Web scope has a display label of "Bing" in C `patches/personal/arcium-omnibox-panel.patch`, and a test asserts that the Web scope does not build its own URL (C `patches/personal/arcium-omnibox.patch`, `EXPECT_FALSE(BuildCommandSearchUrl(kWebScope, ...))`). So the Web scope follows Chromium's default search provider (INFERRED). Which provider that is in the shipped build is UNKNOWN.
- Scope tabs appear when the input is scrolled up (M `docs/arc-quickstart.md`, "Find and preview pages").
- The bar also lists tabs, Pins, Favorites, and actions such as "Move pin to Research" (C `patches/personal/arcium-omnibox.patch` test strings).
- yaac's command bar is not described in sections 1 to 3, 7 or the parts of 10 and 11 that I read. UNKNOWN for yaac.

### 3.5 Recent-tab switcher

- Hold Control and press Tab to preview recent tabs. Thumbnails come from a decoded cache, and capture happens on request (M `docs/feature-performance.md`; C `patches/personal/arcium-mru-tab-switcher.patch`, `mru_tab_preview_cache.cc`).
- Performance instrumentation is local only. The `Arcium.*` histograms are not UMA uploads, and there is no per-frame sampler (M `docs/feature-performance.md`, "Check normal use"). This is a good pattern for yaac's no-telemetry rule: measure with local histograms, and state that they are not uploaded.

### 3.6 Shelf and auto-archive

- Arcium has no auto-archive in the code I searched (C patch text: no matches for auto-archive or archive timers, INFERRED). Its README lists a "Shelf-inspired alternative to auto-archiving" as planned work (M `README.md`; M `docs/preview-release.md`). No design document or Shelf feature code exists in the repositories. The one added match for "shelf" in the C patches is the icon keyword "bookshelf" in the Phosphor catalogue (VERIFIED by grep).
- yaac's section 7 defines auto-archive with a default of 12 hours and an archive container (Y). Arcium's plan is a reason to hold that design until the Shelf behaviour is described by its owners. The Shelf idea is not documented here, so I cannot say what it does.

### 3.7 Settings and customisation

- Settings are native (SwiftUI) and cover layout, collapse behaviour, navigation placement, per-Space colours, grain, glass, borders, UI scale, and surface materials. Settings contracts are in M `docs/feature-settings-audit.md` (23 KB; headings read) and scenario files in `devutils/scenarios/*.json`. yaac's settings are web UI (Y section 12). The settings model and the contract format are worth studying. The UI does not transfer.
- Initial defaults for a new profile include sidebar layout 2, a hidden address bar, Peek, MRU cycling, Little Arc, and a pastel palette (M `resources/preview/initial-preferences.json`).

### 3.8 Comparison with yaac section 7

| Concept | yaac (Y section 7 and decisions) | Arcium (evidence) | Adopt, avoid, or note |
| --- | --- | --- | --- |
| Profile | Isolated browsing identity, one CEF request context | Chromium profile, cookies shared by its Spaces | Same. No change. |
| Space | Named, themed, one profile, containers `pinned`, `today`, `archive` | Named, themed, one profile, own Pins and folders; no archive container found | Note the difference: archive container is yaac-only so far. |
| Favorites | Per-profile container above Spaces | Per-profile, shared by Spaces (M README) | Same. |
| Pins | In the `pinned` container | Pins keep a home URL, dormant when the live tab closes | **Adopt** the home-URL-plus-dormant rule. |
| Folders | Node kind, nestable | Nested folders in a Space | Same. |
| Split | Node kind with two to four tab children | Pair in a Pin, slot identity; saved split still in progress | Note: slot identity. Decide on pairs or up to four. |
| Auto-archive | Timer, default 12 h, archive container | None; Shelf planned | **Hold** until the Shelf behaviour is specified. |
| Boost | Per-site CSS and JS | None | Not covered by Arcium. |
| Peek | Not in sections 1 to 3, 7 | Implemented, with a deferred sign-in-detour design | **Adopt** the backlog's requirements. |
| Little Arc | Not in sections 1 to 3, 7 | Implemented, one URL per request | Consider, if yaac wants external-link routing. |
| Command bar | Not described | Scopes Omni, Web, Saved, YouTube, ChatGPT | Consider scopes. Check the scope list against the privacy rules. |
| Recent-tab switcher | Not described | Hold-Control MRU with cached thumbnails | Consider. Use local histograms to measure. |
| Tab invariants | Tree-shape rules, checked in model and at load | Seeded sequence tests across three tiers, mutation checks | **Adopt** the seeded-sequence and mutation-check method. |
| Agent grants | Per Space, only Space of a non-default profile (Y section 7) | No agent feature; profile separation only | Different, and yaac's rule is stricter. Keep yaac's. |
| Native settings | Web UI | SwiftUI and AppKit on macOS | Do not transfer. |
| Engine | CEF | Chromium built directly (Helium patches + Arcium patches) | Different engine strategy. Not reusable. |

---

## 4. Arc import

### 4.1 What the importer reads

- Source file: `StorableSidebar.json`. Both path forms appear in upstream Helium's importer (C `patches/helium/core/add-arc-importer.patch`: `source_path_.DirName().Append(kStorableSidebarFilename)` and `source_path_.DirName().DirName().Append(...)`). Arcium's patch sits next to that code. Line 733 of `arcium-arc-import.patch` is unchanged context, not an Arcium addition. Arcium does add a profile-folder read in `arcium-arc-favicon-import.patch` line 538 (`profile_directory.Append(FILE_PATH_LITERAL("StorableSidebar.json"))`). The effective location in the shipped build is UNKNOWN. Ardia reads `<Application Support>/Arc/StorableSidebar.json` (A `packages/import/README.md`, "Which file reads what"). The code resolves the file relative to the selected source path in different ways, so the path depends on what that source path points to. The real location in current Arc is UNKNOWN from these repositories.
- Keys used by the parser (VERIFIED, C `arc_sidebar_snapshot.cc` as added by `arcium-arc-import.patch`; the same keys appear in A `ios/Core/ArcSidebar.swift`):
  - top level: `sidebar.containers[].items`; the Space list at `spaces`, the Favorites container list at `topAppsContainerIDs`;
  - each item: `childrenIds` (children), `title`, `data` with one of `tab` (`savedURL`, `savedTitle`), `list`, `tabGroup` (`title`), `itemContainer`, `splitView`; collapsed state from `collapsed` or `isCollapsed`;
  - each Space: `title`, `containerIDs` (keyed by kind, `pinned` matters), `profile` (`default` flag, or `custom.directoryBasename`).
- Output: an `ArcSidebarSnapshot` with `version`, `source_profile`, `favorites`, `spaces` (each with `pins`). Nodes are kinds `tab` and `folder` (C `arc_sidebar_snapshot.cc`, `ToDict`).
- Ardia's Arc parser reads the same keys, and also a theme blob per Space, bounded in depth and size (A `ios/Core/ArcSidebar.swift`, `ThemeBudget`). Its test verifies the theme is kept verbatim (A `tests/ArcThemeTests.swift`).

### 4.2 Mapping into Arcium

- Spaces map to Arcium Spaces, one per Arc Space, bound to the Arcium profile chosen in the import dialog (M `docs/import-from-arc.md`, "What comes over").
- Pinned containers map to Pins. Favorites map to Favorites tiles. Folders keep order and nesting. Stable Arc item IDs are kept as `source_id` (C `arc_sidebar_snapshot.cc`).
- A source profile with Favorites and no Spaces gets an "Arc Favorites" Space so the items are visible (M `docs/import-from-arc.md`; M `docs/releases/0.2.0-preview.1.md`).
- Tabs mode (sidebar as live tabs and groups) is a second option, with a separate reconcile path (C `patches/personal/arcium-arc-import-result.patch`; M `docs/import-from-arc.md`, "Tabs mode must replace prior imported tabs").
- History imports through the standard Chromium importer path and appends visits on each run (M `docs/import-from-arc.md`, "Importing again").
- Passwords and cookies are not in the native dialog. A separate Python tool, `devutils/import_arc_credentials.py`, decrypts Arc's records in memory and re-encrypts them for Arcium's storage key. It needs both browsers closed, requires explicit profile mappings, and writes a backup before applying (M `docs/arc-daily-driver-migration.md`; M `devutils/import_arc_credentials.py`, not read in full).

### 4.3 Known gaps (from the import documentation)

- Split views import as separate pages in order. The split layout is lost (M `docs/import-from-arc.md`, "What comes over"; M `docs/preview-release.md`).
- Favorites folders flatten into tiles. Invalid URLs and `arc:` links are excluded. File and extension links may remain as items but may not open (M `docs/import-from-arc.md`).
- A refresh of a Pins import replaces the source-owned Space trees. Local Pins and folders inside those trees, and edits to saved home pages, are lost. The docs say "Back up Arcium before refreshing" (M `docs/import-from-arc.md`, "Importing again"). This is the most important gap. A user who maintains a local copy inside an imported Space loses it.
- Favicons come from Arc's favicon database when present. A missing icon may need a page visit (M `docs/import-from-arc.md`).
- An Arc profile is an account boundary. Matching names such as "Default" or "Profile 1" do not establish that two profiles are the same account (M `docs/import-from-arc.md`).

### 4.4 Comparison with yaac's recorded knowledge and with ardia

- yaac's section 13 lists Arc as one importer. Section 7 gives the Arc node shape and lists fields that the two parsers do not read (2.4).
- Ardia's importer is a reference for structure, not for copying. Its sources are Apache-2.0 (A `NOTICE`, `LICENSE.Apache-2.0`), the app as distributed is GPL-3.0 because of Helium patches (A `NOTICE`). Its fixtures are invented, and tests refuse to read real profiles (A `packages/import/README.md`).
- What yaac's importer should do, as a design recommendation:
  1. Read `StorableSidebar.json` only from a user-selected Arc profile folder, read-only, and copy it into a temporary location before parsing (ardia does this for SQLite; the same logic applies).
  2. Build the parser from fixtures we write ourselves, using the keys in 4.1 as observed facts, and label them as observed from the owner's files. Do not copy Arcium or ardia code.
  3. Import into a new, separate Space or Space set, with the Arc IDs stored as `origin` fields. Never replace nodes the user has edited. A refresh adds new items and marks removed ones as "gone from Arc" instead of deleting them.
  4. Keep yaac's `Principal` and store rules: importer output goes through the core command API (Y `docs/architecture.md` sections 2 and 3).
  5. Keep the password and cookie path out of the first importer, as Arcium does, and keep it as an explicit later step.

---

## 5. Agent interface and the project's own workflow

### 5.1 "Browser control for agents"

- The README lists it as planned: "Browser control for agents. Build an efficient interface for agents to operate the browser." (VERIFIED, M `README.md`, "What's next"). The release notes repeat it as planned (M `docs/preview-release.md`).
- It is not in `docs/feature-backlog.md`. The backlog has the four shipped items and the Peek sign-in concept, and no agent item (VERIFIED).
- Code: I searched added lines in both patch sets for MCP, remote debugging, DevTools ports, CDP, and LLM provider names. The only hits were false positives (`InstallMotion`, `gemini` as an emoji name, `chatgpt.com` as a URL route). There is no agent control surface in the product code (VERIFIED, grep).
- The existing automation surface is Helium's AppleScript command for selecting a tab (M `patches/helium/macos/applescript-tab-select-command.patch`, in the M series). It is inherited, not new.
- Design intent: none beyond the one line in the README. UNKNOWN.

### 5.2 "Agent" in this project means coding agent

- Every "agent" reference in M's `AGENTS.md`, `docs/agent-workflow.md`, and `docs/parallel-work.md` means an AI coding agent working on the repository. `he agent <command>` is a guard prefix for `he` commands (VERIFIED, M `docs/agent-workflow.md`, "What the prefix adds").
- The yaac team should not confuse the two when reading the repository.

### 5.3 Project workflow: what is useful to yaac

- Patch ownership and build lanes. One build directory has one integrator. Workers use copy-on-write "scratch" trees for patch work, and cannot build (M `docs/parallel-work.md`, `docs/development-changes.md`, "Scratch source for patch authoring"). This is a good model for parallel agent work in yaac's worktree rules.
- Strict push. Patches that apply only with fuzz stop the push, so a bad placement cannot silently land (M `docs/development-changes.md`, "Strict push").
- Proof gates. Each result is reported separately as Inspected, Replayed, Compiled, Packaged, Launched, Behavior-tested, Persistence-tested, Teardown-tested, Manual-only (M `docs/agent-workflow.md`, "Report proof gates separately"). Adopt this. It makes claims like "it runs" checkable.
- Scenario contracts. `devutils/scenarios/*.json` define the steps, profile, and expected behaviour for each feature (M `devutils/scenario_contract.py`, `devutils/tests/test_scenario_contract.py`). yaac could use the same idea for its agent-interface acceptance tests.
- Visible-change checks by a separate computer-use agent. M's workflow says visible changes are verified with a GPT-6.1-Sol computer-use agent and its findings reported (M `docs/agent-workflow.md`, "Runtime testing"). This is a process choice, not code. yaac's rules (Y `CLAUDE.md`) require independent review of the diff, not a visual check.
- Commit and PR rules. Commits are titled in plain words, with no conventional-commit prefix (C `CONTRIBUTING.md`, "Git style"). Multi-commit features merge with `--no-ff` (C `CONTRIBUTING.md`).
- Upstream rule: "Never push to an upstream repository unless Daniel asks." (C `AGENTS.md`, M `AGENTS.md`.) "Daniel" is the owner (INFERRED from the backlog path `/Users/daniel/...` and the commit authorship).

### 5.4 Comparison with docs/research/05-agent-interface.md

- yaac's note 05 studies Aside, Chrome DevTools MCP, Playwright MCP, agent-browser, browser-use, and WebMCP (Y `docs/research/05-agent-interface.md`, section headings read). Arcium contributes nothing to that design. Its one planned line is the same goal, with no design behind it.
- Arcium's workflow shows how a team with agents keeps a patch stack honest. It does not show how to expose a browser to agents. yaac's design in section 11 of the architecture (token-efficient snapshots, grants, audit log) has no counterpart in Arcium.

---

## 6. Privacy posture and network behaviour

| Item | Arcium behaviour | Evidence | Status |
| --- | --- | --- | --- |
| Telemetry | No product telemetry found. Local `Arcium.*` histograms only, not uploaded. | M `docs/feature-performance.md` | VERIFIED for the histograms; the absence of other telemetry is INFERRED from Helium's inherited de-Googling (C `patches/ungoogled-chromium/`, 71 patches). |
| Update checks | Off by default. A Sparkle feed is configured only when `PROD_MACOS_SPARKLE_ED_PUB_KEY` and `PROD_MACOS_SPARKLE_FEED_BASE_URL` are set. Without them the feed URL is empty, and the runtime check at M `patches/personal/arcium-update-feed.patch` line 161 (`configuredFeedURL.length == 0`) has nothing to check (INFERRED that no check runs). | M `docs/building.md` ("Official build"); M `patches/personal/arcium-update-feed.patch` lines 7 to 8 and 161; M `.github/scripts/github_before_build.sh` | VERIFIED for the configuration. Runtime: not run. The preview has manual updates (M `docs/preview-release.md`). |
| Favicons | Read from Chromium's local FaviconService (`GetFaviconImage`, `GetFaviconImageForPageURL`). Arcium adds no new icon endpoint. Chromium still loads favicons from sites as normal. | C `patches/personal/arcium-omnibox.patch` (lines ~8749 to 8936, `FaviconServiceFactory`) | VERIFIED for the code path. Network behaviour at runtime: not run. |
| Command bar Web search | Uses Chromium's default search provider path (INFERRED). Which provider the shipped build uses: UNKNOWN. | C `patches/personal/arcium-omnibox.patch` (test asserting no custom URL for Web scope) | UNKNOWN for suggestion requests. |
| YouTube, X, ChatGPT scopes | Build URLs to those sites. Requests only happen if the user chooses that scope. | C `patches/personal/arcium-omnibox.patch` | VERIFIED (URL construction). |
| Search suggestions | Not reviewed. | | UNKNOWN. |
| First-run "optional online services" | The installer text says first-run explains them. I did not find the text. | M `docs/preview-release.md`, "Install" | UNKNOWN. |
| LLM features | None in the product code. "Local LLM tab cleanup" is planned only. | M `README.md`, "What's next"; grep of added code (no hits) | VERIFIED absence in code. |
| Network call inventory | No equivalent of yaac's `docs/network-calls.md` found. | M and C tree | VERIFIED absence. |
| Bundle identifier | `net.imput.arcium.installer` (Helium's publisher namespace). | M `resources/preview/Installer-Info.plist` | VERIFIED. |
| Ungoogled-chromium de-Googling | Inherited. The ungoogled set includes `disable-ai.patch`, `disable-gaia.patch`, `disable-domain-reliability.patch`, `block-trk-and-subdomains.patch`, `disable-webrtc-log-uploader.patch`, and similar. | C `patches/series`, `patches/ungoogled-chromium/` | VERIFIED file names. Effect at runtime: not run. |

Compared with yaac's rule: "every outbound network call must appear in `docs/network-calls.md`" (Y `CLAUDE.md`, and section 10 of the architecture). Arcium has no such inventory, and its update feed is opt-in (good), but its command bar can reach third-party search and site URLs (user-initiated, fine). For yaac, the one thing to take is the explicit "off until configured" update switch.

---

## 7. Project health and governance

| Measure | Arcium-macOS (M) | Arcium core (C) | Source |
| --- | --- | --- | --- |
| Created | 2026-08-21 (repo) | 2026-08-21 (repo) | `gh repo view` |
| Full history | 1,287 commits, from 2019 (inherited Helium and ungoogled lineage) | 4,076 commits, from 2015 (inherited) | blobless clone, `git rev-list --all --count` |
| Commits since 2026-08-20 | 416, of which 415 by `dr-baker` and 1 by "Daniel Baker" | 310, of which 308 by `dr-baker` and 2 by "Daniel Baker" | `git log --since` |
| Authors overall | `dr-baker` 419, Qian Qian "Cubik" 202, `jj` 193, wukko 172, Eloston 82, helium-bot 65, Zoraver Kang 36, Saltama 30 (raw names, including inherited history) | `Eloston` 1,327, wukko 821, `jj` 543, Blaise 331, `dr-baker` 310, xsmile 113, Zoraver Kang 76, PF4Public 53 | blobless clone |
| Issues ever (non-PR) | 0 | 0 | `gh api`, filter on `pull_request == null` |
| PRs | #1 closed (release notes), #2 open (command bar defaults) | #1 closed, #2 closed, #3 open (command-bar caches) | `gh api` |
| Releases | `0.2.0-preview.1`, pre-release, 2026-10-23, 11 assets | none | `gh api` |
| Stars, forks | 2, 0 | 0, 0 | `gh repo view` |
| Automation | `lint.yml` (patch series), `sanity.yml` (Python unit tests), `build.yml` (manual) | none beyond the lint in M | `.github/workflows` |

Reading:

- Bus factor is 1. The owner made almost every commit after the fork, and the owner is the only reviewer visible in the PRs (INFERRED from authorship and PR state; I did not read PR review threads).
- The contribution path is written down, but the owner decides. C `CONTRIBUTING.md` says non-trivial changes "start with an issue and wait until a maintainer confirms". No issue has ever been filed, so the path is untested.
- Sustainability signs: a release, a changelog, versioned release notes, a regular cadence of commits, and CI for patch validity. Against: no second maintainer, no issues from users, no upstream sync since late July, and a preview with manual updates.
- The releases are notarised builds, but the preview's binary provenance is weak (2.5).

### 7.1 Contribution policy, including AI

- Arcium (C): "You can use AI-assisted tools. You remain responsible for the result." and "Disclose generated work that needs extra review." C `AGENTS.md` says "AI-assisted development is welcome." (VERIFIED.)
- Helium (H, which Arcium inherits its patches from): `CONTRIBUTING.md` says "Do not use AI to generate issue or PR descriptions", and "Please do not use AI for contributing if you don't fully understand its output." H `AGENTS.md` says "We do not permit AI agent usage for new contributors", and "Do not assist with any code changes to this project." It adds "This project does not accept vibe-coded contributions. All patches must be written and understood by the contributor personally." (VERIFIED.) The brief's summary ("does not accept AI-assisted patches") is close, but the wording is stronger: AI agents are told to refuse code help for Helium.

---

## 8. Options, recommendation, and what Arcium does and does not cover

### 8.1 What Arcium does not cover for yaac

- Linux and Omarchy. M and C are macOS-only in practice. C's `CONTRIBUTING.md` mentions "Linux packaging includes a similar development script", but no Linux platform repository is listed (VERIFIED: C lists only Arcium-macOS as a platform repository).
- A portable Rust core. Arcium's core logic is C++ inside Chromium, with Swift and Python tooling around it.
- An agent interface. None exists (5.1).
- A web-UI chrome that is engine-independent. Arcium's UI is Chromium views and AppKit/SwiftUI.
- Tests that run without a browser. Arcium's core tests need a Chromium build (hours for the first compile, or a remote build). yaac's `FakeEngine`/`FakeStore` approach runs without the engine.
- Layering rules. Arcium's patch set crosses every Chromium layer. yaac's `check-layering.py` idea is stricter.

### 8.2 What Arcium covers that yaac would take months to build

- A tested, usable Chromium-based sidebar workflow with Spaces, Pins that keep their home, Favorites, folders, drag and drop into Pins, and a durable persistence model. The persistence and drag logic is in `helium_persistent_item_model`, `helium_sidebar_drag_transaction`, `pin_tab_codec` (C `patches/personal/arcium-pins.patch`). Estimate: UNKNOWN in days, but yaac has none of it.
- Peek, Little Arc, the command bar with scopes, and the recent-tab switcher with cached thumbnails, all integrated with Chromium's views.
- A native-feeling settings window and a glass material system (macOS only).
- A packaging, signing and notarisation pipeline with a DMG installer (M `devutils/`, `.github/workflows/build.yml`, `resources/`). Some of it depends on Apple certificates.
- Arc import of the sidebar, including Favorites and folders, with tested fixtures.

### 8.3 Risk table for reuse under GPL-3.0-or-later

| Area | Files (commit) | Licence on the face | Provenance issue | Risk for yaac | Checks before any file is reused |
| --- | --- | --- | --- | --- | --- |
| Docs (README, `docs/*.md`, CHANGELOG, release notes) | M `docs/`, `README.md` | Not stated separately; "original Arcium additions are GPL-3.0" (M README) | Describe behaviour; do not say how Arc was observed | Low for behaviour notes written by us. Medium if prose is copied (copyright in the text). | Write our own notes from the behaviour. Do not copy sentences. Cite the doc as evidence. |
| Chromium patches, Arcium-authored (`personal`) | C `patches/personal/`, M `patches/personal/` | GPL-3.0 (GPL-3.0-only wording in headers) | Built on Chromium and Helium; uses Chromium APIs | High for direct reuse: the code targets Chromium views, not CEF. Licence: the header wording is narrower than yaac's "or later". | The owner decides the licence question first (section 9). Then check each file's header, its base (Chromium BSD-3, Helium GPL-3, ungoogled BSD-3). |
| Chromium patches, inherited (`helium`, `ungoogled-chromium`, others) | C `patches/helium/`, `patches/ungoogled-chromium/`, etc. | Helium GPL-3; ungoogled BSD-3 (C `LICENSE.ungoogled_chromium`); others per upstream | Upstream code | Medium. Usable under GPL-3, but the patches are Chromium-specific. | Use the upstream projects directly if needed. Check the vendor's own licence. |
| Native Swift UI (SettingsUI, `arcium_settings_swift`) | M `patches/personal/arcium-settingsui-*`, `arcium-native-settings-macos.patch` | None found for SettingsUI; final text names a commit only | **Origin and licence UNKNOWN** (GitHub commit-hash and owner-repository searches found nothing). | **Blocking.** Do not reuse. | Written confirmation from the owner of the origin and licence. Not a reuse candidate in any case (macOS-only, native). |
| ColorTokensKit and ColorSpaces-derived colour files | M `patches/personal/arcium-settingsui-*` | MIT (ColorTokensKit, retained then removed); ColorSpaces licence not read | Removed in the final tree | Low now (not in the final tree). Still, do not reuse without the upstream licences. | Get the upstream licence texts. Not needed for yaac's design. |
| Importer (Arc sidebar) | C `patches/personal/arcium-arc-import.patch`, `arcium-arc-import-result.patch`, `arcium-arc-favicon-import.patch`; upstream `patches/helium/core/add-arc-importer.patch` | GPL-3.0 (Helium Authors header) | Reads Arc's private schema. Method of learning not stated. | Medium. The schema is a fact about user files. The code is copyrightable. | Re-implement from our own fixtures. Do not copy the parser. Ask the owner how the schema was learned. Check ardia (Apache-2.0 sources) only as a structure reference. |
| Python import and tooling | C `devutils/`, M `devutils/` (for example `import_arc_credentials.py`) | GPL-3.0 (headers in C `devutils/` files) | Decrypts Arc data with Keychain keys | Medium-high for the credentials path (crypto and secrets). Low for test patterns. | Do not reuse the credential path without a separate security review. Test patterns: rewrite. |
| Glass prototype (Metal, Swift) | M `devutils/glass-lab/` | No header; repository LICENSE only | Provenance UNKNOWN | Medium. Prototype, not shipped. | Do not reuse without confirming authorship. |
| Glass material in Chromium | C `patches/personal/arcium-glass-system.patch` (`glass_surface*.cc/.mm`), `cc/paint/glassmorphic_paint_filter.h` | GPL-3.0 (Helium Authors) | Chromium-internal APIs | High (not portable to CEF/Linux). | Not a reuse candidate. Use the idea only if we ever need glass. |
| Icons (Phosphor) | C `patches/personal/arcium-phosphor-icons.patch` (`third_party/phosphor-icons/`) | MIT (LICENSE in the patch) | None | Low. Check the version and copy the MIT notice. | Use the upstream package directly, with its licence. |
| Emoji catalogue | C `devutils/generate_space_emoji_data.py` | GPL header on the generator; data from CLDR | Data licence (Unicode) | Low to medium. | Regenerate from CLDR ourselves. Do not copy the output. |
| App icon and logo | M `resources/assets/` | None | Origin UNKNOWN | Medium. Do not reuse. | Make our own icon. |
| Screenshots | M `docs/assets/*.png` | None stated | Show Arcium's UI | Do not reuse. | Take our own. |
| Tests and fixtures | C `*_unittest.cc`, `devutils/tests/`, M `devutils/tests/` | GPL-3.0 | Mostly synthetic (`example.test`) | Low to medium. Test ideas are not copyrightable; test code is. | Rewrite against our own API. |

### 8.4 Options

**(a) Stay separate.** Arcium is a product built on Chromium views, with a single maintainer, macOS only, and a patch stack that is five milestones behind upstream Helium. It does not solve yaac's core problems (CEF, Linux, portable core, agent interface). Cost: none. Benefit: no licence or upkeep coupling. Risk: we miss behaviour lessons unless we write them down, which this note does. **Recommended as the baseline.**

**(b) Collaborate or contribute upstream.** Arcium accepts AI-assisted contributions (C `CONTRIBUTING.md`), and it needs an issue first. Contributions would go into Chromium patches that yaac does not use, into a repository with one maintainer and no issues in its history. The useful contribution would be a report: the Arc importer's refresh behaviour (4.3), or the SettingsUI provenance question. Helium's policy (H `AGENTS.md`) forbids AI agents from helping with Helium contributions, so yaac cannot contribute to Helium with its current workflow. **Not recommended now.** A short, written question to the owner about SettingsUI and the Arc schema is worth sending, since it is needed for any reuse decision.

**(c) Reuse documentation or behaviour notes.** Behaviour notes (section 3) are fine to use as requirements, written in our own words, with the Arcium doc cited as the observation source. Adopt: Pin home URL and dormant state; Peek sign-in requirements; seeded invariant tests; proof-gate reporting; scenario contracts; local-only performance histograms; the "off until configured" update rule. **Recommended.**

**(d) Reuse code.** Not recommended. The Swift UI origin and licence are unknown (blocking). The Chromium UI code does not fit CEF or Linux. The importer copies a private schema with a method we cannot verify. The header licence is GPL-3.0-only wording. The only clean candidate is MIT Phosphor icons, and we can take those from upstream.

### 8.5 Recommendation

Stay separate (a), use behaviour notes (c), and do not reuse code (d). Do not contribute to Arcium (b) until the owner has answered the licence and provenance questions in section 9. Keep Helium out of yaac's contribution path under its current AI policy.

### 8.6 Trade-offs, stated plainly

- Arcium shows that a small team can ship a polished Chromium sidebar with Spaces and Pins on macOS, with a real import path, in about two months from the repository's creation (2026-08-21) to its first preview (2026-10-23). That is evidence that the product idea is worth the effort. It is also evidence that the work is large and macOS-specific.
- yaac's CEF and web-UI choice gives portability and a testable core. It costs us the Chromium UI work Arcium already has. That cost is real, and months of it are plausible.
- Arcium's weakest points for us are the same as its strongest: one maintainer, Chromium-internal patches, and an engine that is five milestones behind.

---

## 9. Open questions for the owner

1. Licence: should yaac's files be GPL-3.0 or GPL-3.0-or-later? `decisions.md` says GPL-3.0, `architecture.md` says GPL-3.0-or-later. Any Arcium file copied in would make this matter.
2. SettingsUI: who is the author and what is the licence of the vendored SettingsUI package? The final patch text names only commit `b8f1fc81`. A GitHub commit search and a name search under `dr-baker` found nothing. Until this is answered, no part of `arcium_settings_swift` may be reused.
3. Arc schema: how was `StorableSidebar.json`'s structure learned? From the owner's own files, from Helium's importer, or from elsewhere? Does yaac's clean-room rule accept "observed in the owner's own Arc profile, with synthetic fixtures written by us" as the evidence standard? The yaac brief's field list (`parentID`, `createdAt`, `isUnread`, `originatingDevice`) is not read by either parser. Is that field list from an observed file, or from memory?
4. Trademarks: are "Little Arc", "Peek", "Shelf", and "Boost" names to avoid? yaac's section 7 uses "Boost". The decision on names belongs to the owner.
5. The importer's refresh replaces source-owned trees. Does yaac want non-destructive refresh as a requirement? (Recommended: yes.)
6. Should yaac's auto-archive design wait for Arcium's Shelf idea, or go ahead with the 12-hour default in section 7?
7. Upstream strategy: yaac cannot contribute to Helium under the current AI workflow (H `AGENTS.md`). Is yaac going to keep its own patch set for CEF, separate from Helium? (Consistent with `decisions.md`, which picks CEF.)
8. Do we want to ask Arcium's owner anything directly (SettingsUI, Arc schema, Shelf), given that the project is open and single-maintainer? The owner's call.
9. Should the Peek sign-in requirements from the Arcium backlog become a yaac requirement? (Recommended for the Peek design.)
10. The Chromium version gap: yaac is on CEF 154 or 156 (Y `docs/research/01`). Arcium is on 151 and Helium upstream on 156. Does that change the research 01 verdict? (Probably not, but record it.)

## 10. What I could not verify

- Arc's own on-disk format, file location, and version history. I read only the parsers and docs in these repositories, and ardia's parser. Nothing was checked against a real Arc install.
- The product's runtime behaviour, network traffic, search-provider defaults, first-run text, and performance numbers. Nothing was built or run, as the rules require.
- Whether the Chromium 156 upgrade would be a small or large job for Arcium. The cost is UNKNOWN without a build.
- Image content: I did not look at Arcium's screenshots or icons for Arc marks, and did not compare the logo with any Arc asset.
- Full reading of `docs/native-settings-bindings.md` (40 KB), `docs/feature-settings-audit.md` (23 KB), and `devutils/import_arc_credentials.py`. I read headings or parts.
- PR review threads and issue discussions (there are none for issues).
- Whether ColorSpaces (the upstream of some colour code) carries a licence I could confirm.
- The separate public repository `dr-baker/mru-tab-switcher` (created 2026-07-23; GitHub reports no licence) was not read. Its link to Arcium's MRU code is UNKNOWN.
- Contributor identities beyond their GitHub-visible names. "Daniel" is inferred to be `dr-baker`.
- Whether any Helium patches in C are Arcium-modified. I read names and headers, not every hunk.
- The yaac architecture sections I did not read (4, 5, 6, 8, 9, 12 to 16) may cover command bar, recent-tab switching, and Peek. I describe them as UNKNOWN for yaac.
