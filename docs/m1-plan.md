# M1 plan: macOS walking skeleton

Goal (from `docs/architecture.md` section 15): a window with a sidebar tab tree, tab open, close and switch, persisted across restarts, on a stock prebuilt CEF (`cef =154.5.0`, run with `--disable-extensions`). Acceptance also includes: swapping the visible content view on a tab switch, E-IPC-1 (a remote page cannot reach `app://ui/api`), and A7 (two request contexts isolate cookies and storage). The last two run as separate spikes after the engine crate exists.

The shared contracts are in this scaffold: `yaac-model` (ids, nodes, `TreeState`), `yaac-api` (wire types), `yaac-engine` (`EngineHandle`, `EngineCommand`, `EngineEvent`, `FakeEngine`), `yaac-core` (`Store` trait, `Principal`, `CoreHandle`, `spawn`, `FakeStore`), `yaac-platform` (`MainThread`, `Platform`). Stubs are marked `todo!("m1/<branch>")`. A public signature changes only through the integrator (the main session); an agent that needs one stops and reports.

Note: `EngineHandle` is `fn send(&self, EngineCommand)`, not one method per command. That is equivalent to the sketch in the spec and easier to fake and log. The spec is updated to match.

| Branch | Owns | Does |
|---|---|---|
| `m1/model` | `crates/yaac-model/**` | `Position`, `TreeState`, invariants, property tests |
| `m1/store` | `crates/yaac-store/**`, `crates/yaac-core/src/testing/store_suite.rs` | SQLite `Store`, migrations, conformance suite |
| `m1/core` | `crates/yaac-core/src/**` (except `testing/store_suite.rs`) | actor, commands, reconcile, events, tests against fakes |
| `m1/ui` | `ui/**` (except `ui/src/generated/`) | sidebar tab tree, bridge, mock bridge, tests |
| `m1/cef` | `crates/yaac-cef/**`, `crates/yaac-app/**`, `crates/yaac-platform/**` | CEF `EngineHandle`, `app://ui` scheme and RPC, macOS host, bundle, selftest |

Shared files (workspace `Cargo.toml`, `Cargo.lock`, `.github/`, `docs/`) belong to the integrator. Merge order: model, store and core, ui, cef.
