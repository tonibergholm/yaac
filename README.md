# yaac

An open-source browser with Arc's usability and Helium's privacy.

Status: early research. Nothing runs yet.

## Direction

- Engine: Chromium through CEF, with Helium-style de-Googling.
- Rust core for tabs, spaces, sidebar tree, command bar logic, storage, sync, importers, settings and IPC.
- Browser chrome is web UI (HTML/CSS/TS) in a CEF view, talking to the core over IPC.
- Platform code sits behind a narrow trait boundary. Targets: macOS (arm64) first, then Linux (x86_64, arm64).
- No telemetry. No AI features in v1.

Research notes are in `docs/research/`. Decisions are in `docs/decisions.md`.

## License

GPL-3.0-or-later, because the Helium patch set requires it. Arc is proprietary: this project observes its behavior and copies none of its code or assets.
