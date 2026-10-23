# yaac architecture

Status: draft 1, 2026-10-23. Written before any production code. Evidence comes from `docs/research/01` to `06` and `docs/spikes/`. Claims carry their evidence level: **observed** (seen in a spike run), **sourced** (read in a primary source, not run), **design** (our decision), **open** (needs an experiment). Open items are collected in section 15.

## 1. Goals and constraints

yaac is an open-source browser with Arc's usability and Helium's privacy. macOS arm64 is the first target. The first Linux target is Omarchy (Arch, Hyprland, Wayland); other distros come later. Decisions already taken are in `docs/decisions.md`. The ones that shape this document:

- Engine: Chromium through CEF, with Helium-style de-Googling. We write no engine.
- Rust owns everything except the engine and the UI markup. The browser chrome (sidebar, command bar, settings) is web UI in a CEF view, talking to the Rust core.
- Platform code sits behind a narrow trait boundary. Everything else is platform-neutral.
- No built-in AI. External agents get a first-class CLI and MCP interface that is token-efficient and safe by default.
- No telemetry. Every outbound network call is listed and justified (section 10).
- GPL-3.0-or-later. Clean room with respect to Arc and Aside: observed behavior and documented interfaces only.

Non-goals for v1: sync, Windows, mobile, a built-in assistant, extension store integration, our own ad blocker engine.

## 2. System overview

```
                    +---------------------------------------------+
                    |               yaac app process              |
                    |                                             |
 CEF UI thread      |  main thread: CEF message loop, windows     |
 (main thread)      |     |  ^                                    |
                    |     v  | EngineEvent / post_task            |
                    |  +-----------------+     +---------------+  |
                    |  | yaac-cef        |<--->| yaac-core     |  |
                    |  | (Engine impl)   |     | (state actor) |  |
                    |  +-----------------+     +---+-------+---+  |
                    |        ^   ^                 |       |      |
 CEF views          |        |   | app://ui        |       |      |
                    |  sidebar view  content views |   yaac-store |
                    |  (web UI)      (web pages)   |   (SQLite)   |
                    |                              |              |
                    |                       +------+-------+      |
                    |                       | yaac-agent   |      |
                    |                       | socket server|      |
                    |                       +------+-------+      |
                    +------------------------------|--------------+
                                                   | Unix socket, 0600 in 0700 dir
                          +------------------------+---------------+
                          |                                        |
                    `yaac <cmd>` (CLI)                       `yaac mcp` (stdio MCP)
                    separate process                         separate process
```

Three kinds of process:

1. The **app process** hosts CEF and the Rust core. CEF's browser process and our core share it. Its main thread runs the CEF message loop.
2. **CEF helper processes** (renderer, GPU, utility) are launched by CEF from the same bundle. macOS needs a helper app bundle layout (observed in the prototype).
3. **Client processes** (`yaac` CLI and `yaac mcp`) are thin clients. They hold no browser state and talk to the core over a local socket.

The sidebar and every tab's content are CEF browser views in one CEF window (CEF Views framework). Observed: one `CefWindow`, a horizontal box layout, a 240 px sidebar view and a content view, both running (prototype, `cef =154.5.0`).

## 3. Crate layout

One Cargo workspace. Dependencies point downward only.

```
crates/
  yaac-model      Pure domain types and tree operations. No I/O, no async.
  yaac-api        Wire types shared by UI, CLI and MCP: Command, Query, Event, errors.
                  Generates the TypeScript types for the UI (design: ts-rs or specta).
  yaac-engine     Traits that describe what the core needs from a browser engine:
                  EngineHost, BrowserHandle, PageDriver. Includes FakeEngine for tests.
  yaac-platform   Platform traits (section 5). macos/ and linux/ modules behind cfg.
  yaac-store      SQLite persistence: schema, migrations, Store implementation.
  yaac-net        Registry of allowed outbound network calls and the check that enforces it.
  yaac-core       State actor, command dispatch, policy and grants, undo, events.
                  Depends on model, api, engine (traits), platform (traits), store (trait).
  yaac-agent      Local socket server, enrollment, scopes, audit log, snapshot shaping.
  yaac-import     Importers: Arc, Chromium family, Firefox, Safari.
  yaac-cef        Implements yaac-engine on cef-rs. The only crate that names CEF types.
  yaac-app        The app binary. Wires everything and owns the main thread.
  yaac-cli        The `yaac` client binary: subcommands and `yaac mcp`.
ui/               TypeScript front end: sidebar, command bar, settings, confirmations.
engine/           (later, probably its own repo) Chromium + Helium + CEF build scripts.
docs/
```

Rules, checked in CI with a script over `cargo metadata`:

- `yaac-model` depends on no other yaac crate.
- `yaac-core` never names `cef`, `objc2`, `rusqlite` or a Linux desktop crate. It sees traits only.
- Only `yaac-cef` depends on `cef`. Only `yaac-platform` implementations depend on OS crates.
- `yaac-cli` depends on `yaac-api` only, so the client stays small and cannot reach state.

Why this shape: the core must be testable without a browser (FakeEngine, in-memory store), and new platforms must cost one `yaac-platform` implementation, not a core change.

## 4. Process and threading model

Constraints, from the prototype and sourced notes:

- macOS: CEF runs its message loop on the main thread, and a Rust-owned loop needs an `NSApplication` subclass that conforms to `CefAppProtocol`. The multi-threaded message loop is not available there (observed in the prototype for the subclass; sourced for the loop limit, `cef_types.h`).
- Quit has to go through CEF's close path, or the process exits non-zero (observed defect, fixed in the prototype).

Design:

- **Main thread**: CEF message loop, window and view code. It runs only engine and platform work. It never blocks on the core.
- **Core thread(s)**: a Tokio runtime. The state actor owns all mutable state and processes one message at a time. This gives one writer and a total order of commands, which makes undo, audit and agent grants simple to reason about.
- **Store thread**: a dedicated blocking thread for SQLite. The actor sends write batches and awaits completion. Reads for the UI come from the in-memory state, not from SQLite.
- **Bridges**: `EngineHandle` posts tasks to the CEF UI thread (`post_task`). Engine callbacks (title changed, load finished, new window requested) become `EngineEvent` values sent to the actor over a channel.

Failure handling: if the core thread panics, the app shows an error view and exits. It does not continue with half-applied state. SQLite writes are transactions; the in-memory state is rebuilt from the store on start.

## 5. Platform trait boundary

Platform code is only what differs by OS. CEF Views already abstracts the window, so the window is not in this list (design, based on the CEF Views decision in `docs/research/02-cef-rs.md`).

```rust
trait Platform: Send + Sync {
    fn paths(&self) -> AppPaths;                    // config, data, cache, runtime dir, log dir
    fn secrets(&self) -> &dyn SecretStore;          // Keychain / Secret Service
    fn notifier(&self) -> &dyn Notifier;            // UNUserNotificationCenter / freedesktop
    fn autostart(&self) -> &dyn Autostart;          // LaunchAgent / XDG autostart
    fn default_browser(&self) -> &dyn DefaultBrowser; // LaunchServices / xdg-settings
    fn file_dialogs(&self) -> &dyn FileDialogs;     // NSOpenPanel / xdg-desktop-portal
    fn app_menu(&self) -> &dyn AppMenu;             // NSMenu / none (in-app)
    fn single_instance(&self) -> Result<InstanceGuard>; // lock + "open this URL in the running app"
    fn lifecycle(&self) -> &dyn Lifecycle;          // activate, quit, dock/tray, window decorations hint
}
```

Per target:

| Trait | macOS | Omarchy / Linux |
|---|---|---|
| `SecretStore` | Keychain | Secret Service (libsecret) |
| `Notifier` | UserNotifications | freedesktop notifications over D-Bus |
| `Autostart` | LaunchAgent plist | XDG autostart file |
| `DefaultBrowser` | LaunchServices | `xdg-settings`, `.desktop` file |
| `FileDialogs` | NSOpenPanel | xdg-desktop-portal, with CEF's own dialog as fallback |
| `AppMenu` | NSMenu (required for Cmd-Q and the standard menu) | no menu bar; commands live in the command bar |
| `single_instance` | Unix socket in the runtime dir | same, under `$XDG_RUNTIME_DIR` |

Linux specifics (sourced in `docs/research/04-cef-wayland-omarchy.md`, partly observed in E2): the app passes `--ozone-platform=wayland` itself, in lowercase (a Wayland session alone does not select it, observed; uppercase crashes, CEF issue #4020). It uses CEF Views, since embedding CEF in a foreign Wayland surface is not available (PR #4233 open). Hyprland forces server-side decoration, so the window may get a compositor titlebar. That is **open**.

Other distros later means: implement the same traits, and add a packaging recipe. A trait method that cannot be implemented on a new platform is a boundary bug, not a reason to leak `cfg` into the core.

## 5a. Engine layer

`yaac-engine` traits (design):

```rust
trait EngineHost {
    fn create_window(&self, spec: WindowSpec) -> WindowId;
    fn create_view(&self, window: WindowId, spec: ViewSpec) -> ViewId;   // sidebar or content
    fn browser(&self, view: ViewId) -> Box<dyn BrowserHandle>;
    fn profile(&self, id: ProfileId) -> Box<dyn ProfileHandle>;          // request context
}
trait BrowserHandle {
    fn load(&self, url: &Url); fn reload(&self); fn back(&self); fn forward(&self);
    fn close(&self) -> CloseOutcome;
    fn page(&self) -> Box<dyn PageDriver>;
}
trait PageDriver {                                                      // used by yaac-agent
    async fn snapshot(&self, opts: SnapshotOpts) -> Snapshot;
    async fn act(&self, action: PageAction) -> ActResult;
    async fn eval(&self, code: &str) -> EvalResult;                     // privileged
    async fn screenshot(&self, opts: ShotOpts) -> Image;
    fn console(&self) -> ConsoleStream;
}
```

`PageDriver` is the seam for agents. It is implemented in `yaac-cef` on the DevTools protocol through `ExecuteDevToolsMethod` and `DevToolsMessageObserver`. Whether those reach other tabs, whether `Accessibility.getFullAXTree` works cold, and whether per-profile request contexts isolate cookies are **open** (experiments A1, A4, A7 in `docs/research/05-agent-interface.md`). If DevTools proves unusable, `PageDriver` falls back to injected scripts. Nothing outside `yaac-cef` changes.

### Runtime style

CEF binds two runtime styles per view, `ALLOY` and `CHROME` (sourced). The prototype used ALLOY for both views. Extensions (uBlock Origin MV2) need Chrome-style support, and Chrome-style code in CEF is patch-level (sourced, `docs/research/03-extensions-mv2.md`). Whether one window can mix styles, and whether Chrome-style content views load a built-in extension, is **open**. Until it is settled, the skeleton uses ALLOY for the sidebar and the content, and the `Engine` traits do not expose runtime style. This decision gates the extension work, not the skeleton.

### Engine build

Observed in E1 (`docs/spikes/e1-patch-apply.md`): Helium's patches first, then CEF's. CEF's 117 patches all apply to Helium's Chromium base. CEF applied after Helium rejects 5 patches (archive mode) or 10 (checkout mode). The reverse order rejects 15 Helium patches. Known GN conflict: CEF forces `enable_rlz=true` and Helium's patch sets `enable_rlz_support=false`. Not tested: domain substitution, pruning, GN, a full build.

Until we have our own build, development uses a stock prebuilt CEF from the CEF build index. **A stock CEF is not private.** The prototype saw established connections to Google-owned hosts with local-only pages. Privacy claims start only when the app runs on the de-Googled build and passes the idle-network test (section 10).

The engine build lives outside this repo's Cargo workspace (a separate directory or repo, `engine/`) because it takes hours and tens of GB. The app consumes versioned CEF artifacts.

## 6. Core: commands, queries, events

All state changes go through one API in `yaac-api`, whatever the caller:

```rust
struct Request { v: u16, id: RequestId, actor: Actor, body: Body }
enum Actor { Ui(ViewId), Cli(ClientId), Mcp(ClientId), Importer, System }
enum Body { Command(Command), Query(Query) }
enum Command { OpenTab{..}, CloseTab{..}, MoveNode{..}, SelectTab{..}, PinTab{..}, CreateSpace{..},
               SetSetting{..}, GrantAgent{..}, RevokeAgent{..}, ... }   // exact list in crates, not here
```

The actor handles a request in four steps: **authorize** (policy and grants for the actor), **validate**, **apply** (mutate state, produce engine effects and store writes), **emit** events. Effects are executed after the store write commits, so a crash cannot leave the engine ahead of the saved state.

Properties this buys:

- The web UI, the CLI and MCP cannot disagree, because they call the same commands.
- Agent grants (section 11) are enforced at one choke point. There is no route to state that skips it. This answers the Aside finding that its `repl` path ran outside the Settings rules (`docs/research/06-aside-observed.md`, section 11).
- Every command can be audited, replayed in tests and (later) undone.

**Events.** The core publishes `Event { topic, rev }` after a change. Events carry a revision number, not state. A client pulls changes with `Query::Since(rev)`. This avoids the ordering bug seen in the prototype, where a URL change arrived with the previous title, and it keeps pushes small and coalescable.

**Undo.** Commands that remove or move nodes record an inverse in a bounded history. Closed tabs stay recoverable (see archive in section 7).

## 7. Data model

Arc's sidebar data, observed earlier on disk, stores tabs, folders and split views as nodes in one tree with `id`, `parentID`, `childrenIds`, `title`, `createdAt`, `isUnread`, `originatingDevice` and a typed `data` payload. Spaces carry an id, a profile and container id lists. We use the same *shape* (a typed node tree). This document specifies our own schema and copies nothing else.

Concepts:

- **Profile**: an isolated browsing identity. One CEF request context and cache path (cookies, storage). Design; isolation between contexts is **open** (A7).
- **Space**: a named, themed group of tabs belonging to one profile. Many Spaces can share a profile. A Space has three containers: `pinned`, `today` (unpinned) and `archive`. An `agent` flag marks Spaces agents may be granted.
- **Favorites**: a per-profile container above the Spaces, shared by all Spaces of that profile.
- **Node**: an element in a container's tree. Kinds: `tab`, `folder`, `split` (a group of two to four tabs shown together), `note` later. Folders and splits can nest in folders.
- **Tab lifecycle**: `live` (has a browser view), `suspended` (browser destroyed; url, title, favicon and scroll kept), `archived` (moved to the archive container).
- **Auto-archive**: unpinned tabs not touched for a user-set period (default 12 hours; 24 hours, 7 days, 30 days or off) are archived by a core timer. Pinned tabs never auto-archive.
- **Boost**: a per-site bundle of CSS and JS the user writes, created with an element picker. Stored per profile, injected by the engine layer on matching origins.

SQLite (via `rusqlite`, bundled, WAL, one writer). Sketch, not final:

```sql
CREATE TABLE profile (id TEXT PRIMARY KEY, name TEXT, cache_dir TEXT, created_at INTEGER);
CREATE TABLE space   (id TEXT PRIMARY KEY, profile_id TEXT REFERENCES profile(id),
                      name TEXT, theme TEXT, agent_enabled INTEGER DEFAULT 0, position TEXT);
CREATE TABLE node (
  id TEXT PRIMARY KEY,                 -- UUIDv7
  space_id TEXT REFERENCES space(id),  -- NULL for favorites
  profile_id TEXT REFERENCES profile(id),
  parent_id TEXT REFERENCES node(id),  -- NULL = container root
  container TEXT,                      -- 'favorites' | 'pinned' | 'today' | 'archive'
  position TEXT,                       -- fractional index, sorts within a parent
  kind TEXT,                           -- 'tab' | 'folder' | 'split'
  title TEXT, url TEXT, favicon BLOB,
  state TEXT,                          -- 'live' | 'suspended' | 'archived'
  is_unread INTEGER DEFAULT 0,
  created_at INTEGER, last_active_at INTEGER,
  updated_at INTEGER, origin_device TEXT, rev INTEGER   -- reserved so sync stays possible
);
CREATE TABLE visit   (id INTEGER PRIMARY KEY, profile_id TEXT, url TEXT, title TEXT, ts INTEGER);
CREATE TABLE boost   (id TEXT PRIMARY KEY, profile_id TEXT, origin TEXT, css TEXT, js TEXT, enabled INTEGER);
CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT);                -- typed in yaac-api
CREATE TABLE agent_client(id TEXT PRIMARY KEY, name TEXT, token_hash TEXT, created_at INTEGER, revoked_at INTEGER);
CREATE TABLE agent_grant (client_id TEXT, space_id TEXT, scopes TEXT, expires_at INTEGER);
CREATE TABLE audit_log (id INTEGER PRIMARY KEY, ts INTEGER, client_id TEXT, space_id TEXT, tool TEXT,
                        scope TEXT, origin TEXT, result TEXT, duration_ms INTEGER);
```

Notes: sibling order uses fractional indexing so a move touches one row. Migrations are numbered SQL files applied in one transaction and recorded in `PRAGMA user_version`. Browsing history (`visit`) is our own table, fed from engine events, because CEF exposes no history API for the omnibox. Page content is never stored. The audit log never stores typed values or page text.

## 8. Web UI and the IPC protocol

The UI is TypeScript, built to static files and served from the custom scheme `app://ui/`, registered in the browser and helper processes (observed in the prototype). It is a set of pages: sidebar, command bar, settings, confirmations, agent log.

**Transport (design, validated in part by the prototype):**

- UI to core: `fetch('app://ui/api/v1/rpc', { method: 'POST', body: JSON })`. The scheme handler decodes a `Request`, sends it to the actor and returns the `Response`. Observed: the sidebar's `fetch()` reached the handler, and Rust called `load_url` on the content frame.
- Core to UI: events are pushed with `execute_java_script('window.yaacEvent({...})')` and carry only `{topic, rev}`. The UI then pulls with `Query::Since(rev)`.
- The wire format is JSON, versioned with `v` in every request. Rust types in `yaac-api` are the source of truth. The TypeScript types are generated from them.
- `CefMessageRouter` is bound in cef-rs but unexercised in its repo (sourced). It is the fallback if the scheme path hits a limit. The UI bridge sits behind a `UiBridge` trait, so the choice stays local to `yaac-cef`.

**Authorization of UI calls.** The scheme handler accepts API calls only from frames whose URL is `app://ui/...` and whose view id matches the view that loaded it. Content views get no bridge. Observed: a call from the content view to a sidebar-only endpoint returned 403. That test used a local `app://` page as content. It has not been tested from a real `https` page, which is the real attack case. It is **open** (experiment E-IPC-1: from a remote page, attempt `fetch('app://ui/api/v1/rpc')` and `XMLHttpRequest`, in default and with CORS set).

## 9. Windows, views and tab lifecycle

- One `CefWindow` per browser window. A root panel with a horizontal box layout holds the sidebar view (fixed width, collapsible) and the content area (observed).
- Each `live` tab is a CEF browser in a `BrowserView`. Only the selected tab's view is visible. Switching tabs swaps the visible view. The core keeps the node-to-browser map.
- Suspending a tab destroys its browser and keeps the node. Restoring creates a new browser and loads the saved url. Memory limits and the suspend policy are tuning work after the skeleton.
- Split view, Peek and Little Arc are later features. The model supports them: `split` is a node kind, and Peek and Little Arc are extra windows or overlay views created through `EngineHost`.
- Popups (`on_before_popup`) become new tab nodes in the same Space through a command, not engine-side windows (design; the prototype denies popups).

## 10. Privacy posture and network calls

Defaults: no telemetry, no crash reporting, no usage statistics, no accounts, no Google services. Every outbound call must appear in `docs/network-calls.md` with purpose, trigger, destination, data sent and whether it is user-initiated. The `yaac-net` crate holds the registry for calls made by our code. A call not in the registry fails in tests.

Calls expected in v1:

| Call | Trigger | Destination | Default |
|---|---|---|---|
| Page loads and subresources | user navigation | the sites the user visits | user-initiated |
| DNS | any load | system resolver (DoH only if the user configures it) | system |
| Filter list updates for the bundled blocker | user-enabled, on a timer | the list host the user selects | off until enabled |
| Update check | none in v1. Linux updates through the package manager. macOS is a manual download | none | none |

Everything else must be silent: no component updater, no safe browsing, no domain reliability, no field trials, no metrics, no sync, no translate, no spellcheck service, no suggestions to a search provider unless the user selects one. These come from the de-Googled Chromium build, not from flags alone: the prototype showed established connections to Google hosts even with the usual `--disable-*` switches on a stock CEF.

**Verification** (test, not assertion): a CI and release check launches the app with an empty profile, idles for 60 seconds, and records connections and DNS queries from the whole process tree (`lsof` plus a net log). The expected set is empty. The first run of this test is the next step on the Google-connection finding.

Agent state, grants and the audit log stay local and are never uploaded.

Extensions: uBlock Origin MV2 as a built-in component with a patch that keeps the MV2 gate open (recommended in `docs/research/03-extensions-mv2.md`, sourced, not run). Risks: Chromium may remove the MV2 path; CEF has had no public extension API since 2024; an update path for the bundled extension and its lists must not depend on the Web Store. External extension providers are disabled in our build: on Omarchy a system file under `/usr/share/chromium/extensions/` made stock CEF install an extension (observed, E2). A Rust-side blocker (`adblock-rust`, MPL-2.0) is the fallback, not v1.

## 11. Agent interface

Decisions in `docs/decisions.md` (2026-10-23): no built-in AI; agents get a CLI and an MCP server; token efficiency and safe defaults are requirements; `evaluate` ships behind confirmation; agents act only in Spaces created or marked for agents; confirmation only for sensitive actions; cookies off by default with a per-Space toggle; schema public and versioned; Windows transport deferred.

**Shape.** `yaac-agent` runs inside the app process and exposes the core API to external clients over a Unix domain socket. `yaac <cmd>` and `yaac mcp` are thin clients of that socket. Requests arrive as `Actor::Cli` or `Actor::Mcp` and pass through the same authorize step as the UI.

**Transport and enrollment (R1, R2).**
- Socket in a per-user runtime directory, mode 0700, socket 0600. The server checks the peer uid on each connection (`getpeereid` on macOS, `SO_PEERCRED` on Linux). No TCP listener, not even on loopback. Aside's daemon uses an unauthenticated loopback port, which is the design we avoid (observed in `docs/research/06-aside-observed.md`).
- A client enrolls once. The user approves it in the app (a confirmation page rendered by the trusted UI). The client receives a grant token stored in a 0600 file. Tokens are revocable and short-lived, and renew only while the grant stands.
- A same-user process holding a token can drive the granted Space. Socket permissions do not stop same-user malware. This limit is stated, not hidden.

**Grants and scopes (R3, R4, R13).** A grant is (client, Space, scopes, expiry). A Space maps to one profile. The default agent Space is dedicated. A Space with logged-in state is reachable only through an explicit grant on a Space marked `agent_enabled`. Never the default profile. Scopes: `read` (list, snapshot, text, screenshot, console), `act` (navigate, click, fill, type, press, select), `privileged` (`evaluate`, `download`, and cookies when enabled).

**Confirmation (R5).** Required for any privileged action, and for sensitive actions: payment and credential fields, account settings, and origins on a user-managed list. Rendered by the trusted web UI, never by page content, and enforced in the core. The sensitive-action detector is conservative and its rules are listed in `docs/agent-api.md` (to be written with the schema). First act on a new origin in a Space with logged-in state also prompts.

**Tools (v1 target: 8 plus 2 privileged).** `tabs` (list, open, close, select, within the granted Space), `navigate` (url, back, forward, reload, optional wait), `snapshot`, `act`, `text`, `screenshot`, `console`, `wait`; privileged: `evaluate`, `download`. Descriptions are short and factual, with no policy prose, because policy is enforced by the core (Aside's four-tool schema is about 8.8 KB, mostly one tool's prose; observed). The schema is public and versioned from the first release.

**Page representation and token cost.** `snapshot` returns an indented accessibility tree with short refs (`e1`, `e2`), a `selector` scope, and a diff after actions when the previous snapshot used the same options. Defaults: output capped at 16 KiB, text inside a node capped with an explicit `[+N chars]` marker, and a `truncated: true` result with a cursor or a suggested narrower scope. Refs distinguish "not found" from "stale". Screenshots return a file path in the Space's artifact folder and are sent as an image only on request, resized. Measured baseline for the same pages (raw HTML against Aside's snapshot): example.com 577 B against 270 B, a GitHub repo page 245,228 B against 6,595 B, Wikipedia 415,391 B against the 16 KiB cap. Our targets are the same order of size. We verify with experiment A8 on our own output.

**Page output is untrusted (R9).** Text from pages is wrapped in a labeled boundary and marked `untrusted: true`. This is a cue for the model, not a defense. The defense is architectural: grants, origin scope and confirmations in the core.

**Visibility and control (R6 to R8).** Tabs an agent drives appear in a group for that Space. The Space shows "agent connected" while a grant is live. An append-only audit log records time, client, Space, tool, scope, origin, result and duration, never page text or typed values. A per-Space kill switch revokes every grant and cancels in-flight commands (`yaac agent stop` does the same from the CLI). Click-to-stop latency is not measured (**open**).

**Limits (R12).** Maximum output size per response, a per-client rate limit, and a concurrency cap per Space. Errors carry no stack traces or internal paths (Aside's do; observed).

**CLI behavior.** Unknown subcommands are errors. A failed script exits non-zero. Leaf `--help` prints leaf help. Values go to stdout and status to stderr, with no ANSI when piped. These are the defects observed in Aside's CLI.

**Memory-type tools** that read personal data stay out of the browser server (Aside puts `memory_search` in the same MCP server; observed).

## 12. Settings

Settings are typed keys in `yaac-api` (name, type, default, scope: global, profile or Space), stored in the `setting` table and edited in the web UI. Examples: auto-archive period, sidebar width, cookies-for-agents per Space, sensitive-origin list, filter list sources, agent server on or off (default off). A setting that changes privacy behavior needs an entry in `docs/network-calls.md` or the decision log.

## 13. Importers

`yaac-import` reads from the source's files on disk, read-only, and emits `Command`s as `Actor::Importer`. The Arc importer reads sidebar data (spaces, pinned and today tabs, folders). Chromium-family importers read bookmarks, history and, with consent, passwords through the platform `SecretStore`. Firefox and Safari follow. The existing `ardia` fork's importers are a reference for what exists on disk; verify the license of each file before reusing code (its own files are Apache-2.0, but the app is GPL-3.0).

## 14. Build, test and verification

- **Pure crates** (`model`, `core`, `api`, `store`, `agent` logic): unit and property tests. `core` runs against `FakeEngine` and an in-memory SQLite.
- **Integration**: the app binary with a `--selftest` mode (pattern proven in the prototype: it drives clicks, waits for round trips, prints structured lines and exits 0 only on success), plus the `yaac` CLI as a client.
- **UI**: component tests against a mocked bridge, and E2E through `--selftest`.
- **Network**: the idle-network test in section 10.
- **Agent**: a conformance suite run against `yaac mcp` over stdio: tools/list, grant enforcement (denied calls really fail), confirmation, audit entries, truncation, kill switch.
- **CI**: GitHub Actions builds `macos-arm64` and Linux x86_64 and runs the non-engine tests. Engine builds are not in CI. Linux GUI checks run by hand on Omarchy until a runner exists.
- **Reproducible releases** are a later goal. v1 pins the CEF artifact by hash.

Packaging: macOS app bundle with helper apps (observed layout). Linux: an Arch PKGBUILD, with the user-namespace sandbox (E2 observed `cefclient` runs without `--no-sandbox` using user namespaces on this kernel; `chrome-sandbox` need not be setuid) and the required runtime libraries listed in `docs/spikes/e2-cefclient-hyprland.md`.

## 15. Milestones and open questions

**M0 (done).** Feasibility spikes and research.

**M1: macOS walking skeleton.** Workspace, `yaac-model`, `yaac-store`, `yaac-core` with FakeEngine tests, `yaac-cef` on cef-rs, a window with the sidebar and a tab tree, open, close and switch, persisted across restarts. On a stock prebuilt CEF.

**M2: Linux skeleton on Omarchy.** Same skeleton on Wayland, `yaac-platform` Linux implementations, Arch package.

**M3: agent interface alpha.** `yaac-agent`, `yaac` CLI and `yaac mcp`, `snapshot` and `act`, grants and the audit log. Ahead of the command bar because it is needed daily.

**M4:** command bar, Spaces and profiles, auto-archive, the Arc importer, Boosts, split view.

**M5:** the de-Googled engine build (Helium then CEF), the idle-network test, the extension decision, and the privacy audit.

**Open questions and the experiment that settles each:**

| # | Question | Experiment |
|---|---|---|
| 1 | Does `cefclient` paint on Hyprland, and does Hyprland add a titlebar? E2 was confounded by Omarchy's idle screensaver and lock | Rerun with idle disabled and the user present |
| 2 | What opens the Google connections in stock CEF? | `--log-net-log` on the prototype, then repeat on our build |
| 3 | Can one window mix ALLOY and CHROME views, and do Chrome-style views load a built-in extension? | Extend the prototype; read `cef_runtime_style_t` |
| 4 | Do `ExecuteDevToolsMethod` and `Target.*` reach other tabs; does `getFullAXTree` work cold; does pipe mode work? | A1 to A4 in note 05 |
| 5 | Do two request contexts isolate cookies and storage (Spaces and profiles)? | A7 |
| 6 | Can a remote `https` page reach `app://ui/api`? | E-IPC-1 (section 8) |
| 7 | Do CEF's and Helium's patches build, not only apply? Does domain substitution change the conflicts? | A full build, with the owner's approval; it takes hours |
| 8 | Is CEF 156 stable or beta? | Read the CEF builds index at build time |
| 9 | Kill-switch latency and the indicator on screen | Measure in M3; Aside's indicator screenshot is still open |
| 10 | Snapshot size and tokens on our output | A8 |

## 16. Decisions to record after review

1. One typed command API serves the UI, CLI and MCP; the actor model and single writer.
2. UI to core over the `app://ui` scheme with revision-only events, behind a `UiBridge` trait.
3. The node-tree schema and fractional ordering; reserved sync columns.
4. Helium patches first, then CEF's (already recorded from E1).
5. The `Platform` trait list and the rule that the core never names OS or CEF types.
6. Agent interface scopes, transport and the limits listed in section 11.
