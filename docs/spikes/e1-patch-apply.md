# E1: CEF patches and Helium patches on one Chromium tree

Date: 2026-10-23. Patch-apply test only. No Chromium build, no depot_tools, no gclient sync, no automate-git.py, no domain substitution, no pruning. Run on Omarchy in `/home/toni/yaac-spike/e1/`. Nothing in the yaac repo was edited.

Labels: VERIFIED = observed in this run or read in a primary source. INFERRED = reasoning from verified facts.

## Verdict

- **VERIFIED.** CEF's 117 patches apply cleanly to Chromium 156.0.8078.47 (the Helium base), with no fuzz and no rejects. The 47 vs 48 point-release gap causes no rejects.
- **VERIFIED.** Helium's series applies alone to the same base: 344 of 346 clean, 0 fuzz. The 2 rejects are ublock patches whose target files come from `deps.ini`, which is not in the base. They reject in every pass.
- **VERIFIED.** The two orders collide in different places. CEF first (B) gives 13 new Helium rejects. Helium first (C) gives 5 CEF rejects in patch-tool mode, or 10 in the `git apply` mode that CEF uses on a real checkout.
- **INFERRED.** On this evidence, Helium first is the cheaper order to maintain. It needs fewer manual re-diffs and fewer offset changes. This reverses the research note's recommendation (CEF first). The margins are 5 vs 13 rejects (patch tool) and 10 vs about 17 (strict). This test does not cover GN, domain substitution, or runtime behavior.
- **VERIFIED.** The note's claim that CEF's patcher "aborts on first failure" is wrong for branch 8078. Config mode runs every patch and exits 1 at the end (`tools/patcher.py`, `apply_patch_config`). Helium's own `apply_patches` does abort on the first failure (`check=True`).

## Versions, sources, hashes

| Item | Value | Status |
|---|---|---|
| Chromium base | 156.0.8078.47, from `downloads.ini` in imputnet/helium (commit f29a25968e483f8bb85e47829ccd487bbe229199) | VERIFIED |
| Base URL | `https://tarballs.imput.net/chromium-156.0.8078.47/chromium-156.0.8078.47-base.tar.zst` | VERIFIED |
| Base size | 1,983,006,019 bytes | VERIFIED |
| Base sha256 | `d086d3b8c01b6d192bb54383459979641d2fa57e7c2599b9cb1f70b96c6a10be` (matches `chromium-156.0.8078.47-base.tar.zst.hashes` at `github.com/imputnet/chromium-tarballs/releases/download/chromium-156.0.8078.47/`) | VERIFIED |
| What "-base" is | The CI fallback in `.github/workflows/ci.yml` builds it with `utils/clone.py` (gclient) and then strips `.git`. Assumed to be pre-substitution, pre-pruning. | INFERRED |
| Note correction | The research note guessed `-lite.tar.xz` on commondatastorage. The real Helium pin is `-base.tar.zst` on tarballs.imput.net. | VERIFIED |
| CEF patch set | branch `8078` at 426920c33 (pins `refs/tags/156.0.8078.48`); `patch/patch.cfg` has 117 entries, `patch/patches/` has 118 files. The extra file `chrome_browser_privacy_1119417.patch` is not in patch.cfg and was not applied. | VERIFIED |
| CEF master (for the note's 96/32 check) | 7f8a5ec28e932aa62b583c8248e3df2e4dfe4807 (pins .0) | VERIFIED |
| Helium series | `patches/series` at f29a259: 346 entries, all files present | VERIFIED |
| Helium-linux | 473c0f72d7db2e8492f0fa975b8544bc719bb027, not used (its platform patches are not in the series) | VERIFIED |
| Tools | GNU patch 2.8, git, zstd, python3 (Omarchy) | VERIFIED |

Version drift: the base is .47, CEF branch 8078 pins .48, CEF master pins .0. Only the 8078 patch set was used for the passes.

## Method

1. Downloaded the base with `curl`, verified sha256, extracted with `zstd -dc | tar -x --strip-components=1 -C base/`. Extraction took 46 s, and the tree is 8.1 GB without `.git`. The tree contains the DEPS-managed directories (`v8`, `third_party/angle`, `third_party/dawn`, `third_party/depot_tools`). No CEF patch was skipped for a missing target directory (VERIFIED in pass A).
2. CEF side. The base has no `.git`, so CEF's `tools/git_util.py` takes the **patch tool** path (`_patch_apply_patch_string`), not `git apply` (`is_checkout()` is false). The replica runs `patch -p0 --ignore-whitespace --force` with the patch on stdin, in the patch.cfg `path` directory (default: tree root), after a reverse dry run. Missing target directories are skipped, as CEF does. A second mode (`CEF_MODE=git`) replicates the checkout path, `git apply -p0 --ignore-whitespace --check`, which allows no fuzz and applies all-or-nothing per patch.
3. Helium side. Replicates `utils/patches.py` `apply_patches()`: `patch -p1 --ignore-whitespace -i FILE -d TREE --no-backup-if-mismatch --forward`, in series order, with default fuzz. Helium's tool stops at the first failure. Here every patch runs, so the full reject list is recorded.
4. Each pass ran on a fresh copy of `base/` (`cp -a --reflink=auto`). Status per patch: clean, fuzz-or-offset, rejected, skip-already-applied. The raw patch output of every non-clean patch is in `results/<pass>/output.log`.
5. Overlap: parsed the `---`/`+++` headers and `@@` ranges of both sets, applied the `-p0` and `-p1` path rules, and compared old-side ranges padded by 3 lines (the note's convention). This is INFERRED for conflict risk, not an apply test.

Controls: pass A (CEF alone) tests version drift. Pass H (Helium alone) tests whether Helium's series applies to its own base.

## Results

| Pass | Order | CEF (clean / fuzz-offset / rejected) | Helium (clean / fuzz-offset / rejected) | First new reject (seq, patch) |
|---|---|---|---|---|
| A | CEF alone (patch tool) | 117 / 0 / 0 | n/a | none |
| H | Helium alone (patch tool) | n/a | 344 / 0 / 2 (ublock, deps) | none new |
| B | CEF then Helium (patch tool) | 117 / 0 / 0 | 259 / 72 / 15 (13 new + 2 ublock) | 129, `ungoogled-chromium/disable-gcm.patch` |
| C | Helium then CEF (patch tool) | 86 / 26 / 5 | 344 / 0 / 2 (ublock) | 365, `chrome_runtime_views` |
| A-git | CEF alone (git apply) | 117 / n/a / 0 | n/a | none |
| B-git | CEF then Helium (CEF part git apply; Helium part is patch tool, so identical to B) | 117 / n/a / 0 | 259 / 72 / 15 | 129 |
| C-git | Helium then CEF (CEF part git apply; Helium part patch tool, identical to C's Helium part) | 107 / n/a / 10 | 344 / 0 / 2 (ublock) | CEF rejects start at 348 |

Hunk counts (patch-tool runs): H 24 failed hunks (all ublock); B 39 (24 ublock + 15 new); C 36 (24 ublock + 12 CEF). Git mode counts patches, not hunks.

Fuzz split: B has 4 Helium patches with fuzz (the other 68 are offset-only). C has 5 CEF patches with fuzz (21 offset-only). In git mode the 5 fuzzy CEF patches are exactly the extra rejects (10 = 5 + 5), which confirms git apply refuses fuzz.

## Reject lists

Ublock baseline (both orders, and the Helium-alone control): `helium/core/ublock-setup-sources.patch`, `helium/core/ublock-helium-services.patch`. Targets are under `third_party/ublock/`, which `deps.ini` provides. They are not in the base.

Pass B, 13 new Helium rejects (patch tool; all 13 are inside the predicted overlap set):
- ungoogled-chromium/disable-gcm.patch
- ungoogled-chromium/remove-unused-preferences-fields.patch
- ungoogled-chromium/add-flag-to-show-avatar-button.patch
- ungoogled-chromium/add-flag-for-incognito-themes.patch
- helium/core/search/remove-description-snippet-deps.patch
- helium/core/add-helium-versioning.patch
- helium/core/prioritize-platform-policy-loading.patch
- helium/core/crash-reporter-privacy-tweaks.patch
- helium/core/remove-unused-ai-resources.patch
- helium/ui/tab-search-in-toolbar.patch
- helium/ui/app-menu-model.patch
- helium/ui/custom-keyboard-shortcuts-wiring.patch
- helium/ui/layout/zen-mode.patch

Pass B `.rej` files (14): `chrome/browser/policy/chrome_browser_policy_connector.cc`, `chrome/browser/signin/account_consistency_mode_manager.cc`, `chrome/browser/ui/browser_window/internal/browser_window_features.cc`, `chrome/browser/ui/toolbar/app_menu_model.cc`, `chrome/browser/ui/views/frame/browser_view.cc`, `chrome/browser/ui/views/frame/browser_widget.cc`, `chrome/browser/ui/views/toolbar/toolbar_view.cc`, `chrome/chrome_paks.gni`, `chrome/chrome_repack_locales.gni`, `chrome/common/crash_keys.cc`, `components/crash/core/app/crashpad_linux.cc`, `components/crash/core/app/crashpad_mac.mm`, `components/crash/core/app/crashpad_win.cc`, `components/gcm_driver/gcm_client_impl.cc`.

Pass C, 5 CEF rejects (patch tool): `chrome_runtime_views` (5 hunks), `crashpad_1995` (4), `chrome_browser_policy` (1), `components_gcm_disable` (1), `signin_allowed_4276` (1).

Pass C `.rej` files (10): `chrome/browser/policy/chrome_browser_policy_connector.cc`, `chrome/browser/signin/account_consistency_mode_manager.cc`, `chrome/browser/ui/views/frame/browser_view.cc`, `chrome/browser/ui/views/frame/browser_widget.cc`, `chrome/browser/ui/views/location_bar/location_bar_view.cc`, `chrome/browser/ui/views/toolbar/toolbar_view.cc`, `chrome/common/crash_keys.cc`, `components/crash/core/app/crashpad_mac.mm`, `components/crash/core/app/crashpad_win.cc`, `components/gcm_driver/gcm_client_impl.cc`.

Pass C-git, 10 CEF rejects (git apply, no fuzz): `gn_config`, `webkit_move_pip`, `chrome_runtime_views`, `chrome_browser_browser`, `chrome_browser_frame_mac`, `chrome_browser_policy`, `chrome_browser_webui_version`, `crashpad_1995`, `components_gcm_disable`, `signin_allowed_4276`.

`helium/core/search/remove-description-snippet-deps.patch` is a real conflict, not a duplicate. It deletes the `enable_builtin_search_provider_assets` block in `chrome/chrome_repack_locales.gni`, and CEF's `gn_config.patch` edits the same file inside that block. GNU patch printed "Reversed (or previously applied)" for that hunk, and the hunk was rejected. The exact final file state was not diffed.

The reject sets differ by order, as expected. The same collisions appear in both: GCM (`disable-gcm` in B, `components_gcm_disable` in C) and crash reporting (`crash-reporter-privacy-tweaks` in B, `crashpad_1995` in C).

## Overlap vs the note's prediction (96 files, 32 hunk overlaps)

| Measure | Note (CEF master 7f8a5ec) | Reproduced, master | CEF branch 8078 (used in passes) |
|---|---|---|---|
| CEF distinct paths | 462 | 462 | 462 |
| Helium distinct paths | 1645 | 1645 | 1645 |
| Shared paths | 96 | 96 | 96 |
| Shared directories (directory intersection) | 81 | 81 | not computed |
| Shared paths with overlapping hunks | 32 | 32 | 33 |
| CEF patches involved | not stated | 14 | 14 |
| Helium patches involved | not stated | 29 | 29 |

- **VERIFIED.** Master reproduces the note exactly. Branch 8078 gives 33 overlapping files.
- **VERIFIED.** All 13 new Helium rejects in B and all 5 CEF rejects in C are in the overlap-involved set. The hunk overlap is a reasonable predictor of rejects. It is not a complete one: 16 of the 29 involved Helium patches applied in B.
- Note on the "81 directories" figure: it is the intersection of directories that appear in both sets. Shared files lie in 52 distinct directories.

## Order verdict

- **VERIFIED.** Helium first gives fewer real rejects under both tool choices, compared like with like:
  - Patch tool on both sides (the archive-mode path CEF uses for a tarball tree): Helium first 5 CEF rejects, CEF first 13 Helium rejects.
  - Strict application on both sides (`git apply` for CEF, strict for Helium): Helium first 10 CEF rejects, CEF first about 17 (INFERRED: the 13 measured rejects plus the 4 Helium patches that applied only with fuzz; strict Helium was not run).
  - Helium first also gives fewer offset-only changes (21 vs 68, patch tool).
- **VERIFIED.** The note's reason for CEF first, that CEF's patcher aborts once context moves, is not supported. CEF's config mode does not abort. Helium-first produced fuzzy applications, not a hard stop.
- **VERIFIED (automate-git.py, branch 8078).** Archive mode extracts the tarball, applies `tarball_deps` (DEPS) and `tarball_gclient` (depot_tools gclient.py), runs `gclient sync --nohooks`, then runs CEF's `patcher.py`. The extracted tree has no `.git`, so `is_checkout()` is false and CEF uses the patch tool. No `git init` happens in this path. So the count for Helium-first via `--chromium-archive` is the patch-tool count: 5 CEF rejects. Checkout mode (`git apply`) would give 10.
- **INFERRED.** Helium first via archive mode therefore needs 5 hand re-diffs. The 26 CEF patches that patch-tool applies with fuzz or offset are accepted silently in this path. The 5 with fuzz are the riskiest. Checkout mode would reject them and give 10.
- **INFERRED, maintenance cost.** Helium first means carrying re-diffed CEF patches. CEF's even-milestone cadence is roughly monthly. CEF first means carrying re-diffed Helium patches on Helium's roughly two-week merge cadence. That supports Helium first beyond the raw counts, provided the 5 to 10 CEF re-diffs are acceptable.
- **INFERRED.** The recommendation is Helium first via archive mode (E3), with the 5 CEF patches re-diffed by hand. This is a patch-apply result only. It does not show that the order gives a working build. The tarball patches and `gclient sync` were not run here. Their target files were not checked against the overlap set.
- **Pass A and drift.** Pass A had zero rejects, so the .47 vs .48 gap does not explain any reject in B or C. Pass H's only rejects are the ublock deps, and they explain the two baseline rejects in B and C.

## Limitations

- **Not built.** No GN, no compile, no runtime check. RLZ (E2), Helium's GN flags vs CEF (E6), and the `enable_rlz` assert are untested.
- **No domain substitution, no pruning.** Helium runs both before its series. Domain substitution rewrites strings in many files and can change the context that patches expect. Pruning removes files that patches may touch. Either could add rejects in both orders. This is the largest gap and could change the counts. Direction: INFERRED to add rejects, not measured.
- **Helium platform patches not applied.** Only the shared `patches/series` ran, not `helium-macos` or `helium-linux` platform patches.
- **deps.ini assets missing.** The 2 ublock patches reject in every pass. Other DEPS-managed directories are present.
- **Fuzz accepted in patch-tool mode.** Default fuzz 2 was used for both sides. Helium's own dry run uses `--fuzz=0`, which was not run separately. The git-mode run shows what strict application would reject.
- **Partial application.** Patch-tool mode applies the hunks that succeed and leaves `.rej` files. Downstream results after a reject inherit that partial state.
- **CEF master overlap vs 8078 patches.** The 8078 patch files differ from master's in many places (CEF tracks .48 on 8078). The passes used 8078. Master was used only for the overlap check against the note.
- **One CEF patch orphaned.** `chrome_browser_privacy_1119417.patch` is in `patches/` but not in patch.cfg. Not applied, and it does not change any count here.
- **Clone-based base.** The `-base.tar.zst` is inferred to be a gclient-synced, pre-substitution clone (INFERRED from `ci.yml`). Its exact contents were not compared to googlesource beyond the sha256.

## Timings (Omarchy, 8 cores, nice 19 and ionice idle)

- Download of the base: about 5 minutes (1.98 GB).
- Extraction: 46 s.
- Tree copy per pass: 30 to 38 s (8.1 GB).
- Patch apply per pass: 0.7 to 6 s.
- Overlap script: under 5 s.

## Reproduce

On Omarchy, inside `/home/toni/yaac-spike/e1/`:

```sh
mkdir -p dl base src && cd dl
curl -fsSL -o chromium-156.0.8078.47-base.tar.zst \
  https://tarballs.imput.net/chromium-156.0.8078.47/chromium-156.0.8078.47-base.tar.zst
echo "d086d3b8c01b6d192bb54383459979641d2fa57e7c2599b9cb1f70b96c6a10be  chromium-156.0.8078.47-base.tar.zst" | sha256sum -c -
cd ../base && zstd -dc ../dl/chromium-156.0.8078.47-base.tar.zst | tar -x --strip-components=1 -f -
cd ../src && git clone https://github.com/imputnet/helium.git && git -C helium checkout f29a25968e483f8bb85e47829ccd487bbe229199
git clone --filter=blob:none https://github.com/chromiumembedded/cef.git
git -C cef fetch -q origin 8078 && mkdir -p ../cef8078 && git -C cef archive origin/8078 patch | tar -x -C ../cef8078
cd .. && python3 -I scripts/gen_manifest.py cef8078/patch src/helium/patches manifests/8078
python3 -I scripts/overlap.py manifests/8078/cef.tsv manifests/helium.tsv results/overlap-8078
# per pass: fresh copy of base, then
cp -a --reflink=auto base work/passX
python3 -I scripts/apply_pass.py work/passX manifests/passX.tsv results/passX      # CEF_MODE=git for checkout mode
```

Scripts: `/home/toni/yaac-spike/e1/scripts/` (`gen_manifest.py`, `apply_pass.py`, `overlap.py`, `run_passes.sh`, `run_git.sh`). Results: `/home/toni/yaac-spike/e1/results/`.

## Disk on Omarchy

`work/` held 7 pass copies. `du` reports 67 GB of apparent size, but the btrfs reflink copies share extents, so the real saving is smaller. Measured with `df -B1 /home` (Used column): 177,507,438,592 bytes before, 168,920,571,904 bytes after. **Freed: 8.6 GB** (8,586,866,688 bytes). Other activity on the machine during the window can also move this figure.

Kept: `base/` (8.1 GB, pristine), `dl/` (1.9 GB, the verified tarball), `src/` (82 MB), `cef8078/`, `cef-master/`, `manifests/`, `results/` (482 KB), `scripts/`. The `base/` tree is unmodified. Its 300 `.rej`/`.orig` files are upstream vendored Cargo files in the tarball, dated before the download.
