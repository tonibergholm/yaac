# yaac: working conventions

Open-source Arc-like browser. Chromium via CEF, Rust core, web-UI chrome. Read `docs/architecture.md` first, then `docs/decisions.md`. Research notes are in `docs/research/`, spike results in `docs/spikes/`.

- **Clean room.** Arc and Aside are proprietary. Use observed behavior and documented interfaces only. Never copy or paste their code, prompts or assets.
- **No telemetry.** Every outbound network call must appear in `docs/network-calls.md` (to be created with the first call). A call that is not listed fails the idle-network test.
- **Layering** (checked by `python3 -I scripts/check-layering.py`): `yaac-model` depends on no other yaac crate. `yaac-core` names no CEF, objc2 or SQLite crate and defines the `Store` trait. Only `yaac-cef` depends on `cef`. Adapters (`yaac-store`, `yaac-cef`, platform implementations) depend on core and model, never the reverse.
- **One command API.** The UI, CLI and MCP all go through `yaac-core`. Authority comes from the ingress (`Principal`), never from the request.
- **Tests.** Core logic is tested against `FakeEngine` and `FakeStore` (`yaac-core` feature `testing`). Run `cargo test --all-targets`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`.
- **Generated code.** TypeScript types in `ui/src/generated/` are generated from `yaac-api`: `cargo test -p yaac-api --features ts`. Do not edit them by hand.
- **Edits happen in git worktrees**, never in the main checkout. Branch from `main`; open a PR; every non-trivial change gets an independent review of the final diff before merge.
- **Evidence labels** in docs: observed (seen in a run), sourced (read in a primary source), design (our decision), open (needs an experiment).
