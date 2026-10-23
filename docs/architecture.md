# yaac architecture

Status: draft 2, 2026-10-23, revised after independent review by `code-reviewer` and Sol (`gpt-6.1-sol`). Written before any production code. Evidence comes from `docs/research/01` to `06` and `docs/spikes/`. Claims carry their evidence level: **observed** (seen in a spike run), **sourced** (read in a primary source, not run), **design** (our decision), **open** (needs an experiment). Open items are collected in section 15.

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
- Only `yaac-cef` depends on `cef`. Only `yaac-platform` implementations and `yaac-cef` depend on OS crates. `yaac-cef` needs `objc2` for the macOS `NSApplication` subclass that conforms to `CefAppProtocol` (observed in the prototype); that adapter lives in `yaac-cef/src/macos/` and exports no Objective-C types.
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
- **Bridges**: `EngineHandle` is a `Send + Sync` command proxy. It carries owned ids and plain data, never CEF objects, and posts work to the CEF UI thread (`post_task`). CEF objects stay on the UI thread. Engine callbacks (title changed, load finished, close completed, new window requested) become `EngineEvent` values sent to the actor over a channel.
- **Split-phase rule**: the actor never awaits the engine or a confirmation. It issues a command, records a pending operation and continues; the result arrives later as an event. Long page operations and confirmations therefore cannot stall a stop or revoke message.
- **Main-thread platform calls**: platform services that need the main thread (menus, open panels, notifications) are reached through the same proxy. The `Platform` methods below are async from the core's side and are marshalled to the main thread by the host.

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

Linux specifics (sourced in `docs/research/04-cef-wayland-omarchy.md`, partly observed in E2): in a real Hyprland session (`XDG_SESSION_TYPE=wayland`, sockets reachable) CEF chose native Wayland with no flag, and with `XDG_SESSION_TYPE=tty` over SSH it fell back to XWayland (observed, E2 runs r2c to r2e). The app still passes `--ozone-platform=wayland` itself, in lowercase, so that launches from a desktop entry or service without the session variable do not fall back (uppercase crashes, CEF issue #4020). It uses CEF Views, since embedding CEF in a foreign Wayland surface is not available (PR #4233 open). Hyprland forces server-side decoration, so the window may get a compositor titlebar. That is **open**.

Other distros later means: implement the same traits, and add a packaging recipe. A trait method that cannot be implemented on a new platform is a boundary bug, not a reason to leak `cfg` into the core.

## 5a. Engine layer

`yaac-engine` is a command-and-event interface, not a set of synchronous object handles (design):

```rust
// Core -> engine. Cheap to clone, Send + Sync. The core allocates every id.
trait EngineHandle: Send + Sync {
    fn create_window(&self, id: WindowId, spec: WindowSpec);
    fn create_view(&self, id: ViewId, window: WindowId, spec: ViewSpec);   // sidebar or content
    fn load(&self, view: ViewId, url: Url);
    fn reload(&self, view: ViewId); fn back(&self, view: ViewId); fn forward(&self, view: ViewId);
    fn show(&self, view: ViewId);                                           // swap the visible content view
    fn close(&self, view: ViewId);                                          // result arrives as EngineEvent::Closed
    fn page(&self, view: ViewId) -> Option<Arc<dyn PageDriver>>;            // content views only
}
// Engine -> core, over a channel. Carries a navigation generation so stale callbacks can be ignored.
enum EngineEvent { Created{..}, Navigated{view, nav_gen, url}, TitleChanged{view, nav_gen, title},
                   LoadState{..}, Closed{view}, PopupRequested{opener, url, user_gesture},
                   DownloadRequested{view, nav_gen, ..}, Crashed{view} }
```

Nothing here returns a result from main-thread work. Creation and close are confirmed by events, because CEF close is asynchronous (observed: the prototype had to run both browsers through `try_close_browser` before quit).

The `PageDriver` interface is object-safe (async methods return boxed futures) and capability-based: `capabilities()` reports what the driver can do, and an unsupported call returns an explicit `Unsupported` result. Methods: `snapshot`, `act`, `eval` (privileged), `screenshot`, `console`. A driver is only ever handed out for content views in a Space the caller is granted. It is never handed out for a UI view (section 11).

`PageDriver` is the seam for agents. It is implemented in `yaac-cef` on the DevTools protocol through `ExecuteDevToolsMethod` and `DevToolsMessageObserver`. Whether those reach other tabs, whether `Accessibility.getFullAXTree` works cold, and whether per-profile request contexts isolate cookies are **open** (experiments A1, A4, A7 in `docs/research/05-agent-interface.md`). If DevTools proves unusable, injected scripts are not a demonstrated equivalent for accessibility snapshots, screenshots or cross-origin frames. The fallback is a driver with fewer capabilities, reported through `capabilities()`, and the tool results say so. Nothing outside `yaac-cef` changes.

### Runtime style

CEF binds two runtime styles per view, `ALLOY` and `CHROME` (sourced). The prototype used ALLOY for both views. Extension support in CEF is patch-level code (sourced, `docs/research/03-extensions-mv2.md`). That note recommends Chrome-style windows for uBlock Origin, but it also records a CEF maintainer view that request-context features such as network interception work under Alloy, and that extension UI and the general Tab APIs differ. So Chrome style is a recommendation, not an established requirement. What uBlock Origin actually needs, whether one window can mix styles, and whether Chrome-style content views load a built-in extension, is **open**. Until it is settled, the skeleton uses ALLOY for the sidebar and the content, and the `Engine` traits do not expose runtime style. This decision gates the extension work, not the skeleton.

### Engine build

Observed in E1 (`docs/spikes/e1-patch-apply.md`): Helium's patches first, then CEF's. CEF's 117 patches all apply to Helium's Chromium base. CEF applied after Helium rejects 5 patches (archive mode) or 10 (checkout mode). The reverse order rejects 15 Helium patches, of which 2 also reject in either order (missing inputs), so 13 are caused by the ordering. Not tested: domain substitution, pruning, GN, a full build. A known GN conflict (CEF forces `enable_rlz=true`; Helium's patch sets `enable_rlz_support=false`) comes from `docs/research/01-cef-on-helium.md` (sourced), not from E1.

**Version pin policy (design).** The skeleton (M1, M2) pins `cef =154.5.0` with the CEF 154.0.34 prebuilt it binds (observed in the prototype). The engine build tracks Helium's Chromium (156.0.8078.47 at the time of E1), and cef-rs trails CEF by one milestone (sourced, note 02). Closing that gap, by a cef-rs bump or our own bindings, is an M5 decision. Until then no feature may depend on a CEF 156 API.

Until we have our own build, development uses a stock prebuilt CEF from the CEF build index. **A stock CEF is not private.** The prototype saw established connections to Google-owned hosts with local-only pages. Privacy claims start only when the app runs on the de-Googled build and passes the idle-network test (section 10).

The engine build lives outside this repo's Cargo workspace (a separate directory or repo, `engine/`) because it takes hours and tens of GB. The app consumes versioned CEF artifacts.

## 6. Core: commands, queries, events

All state changes go through one API in `yaac-api`, whatever the caller:

```rust
struct Request { v: u16, id: RequestId, body: Body }      // no caller identity on the wire
enum Body { Command(Command), Query(Query) }
enum Command { OpenTab{..}, CloseTab{..}, MoveNode{..}, SelectTab{..}, PinTab{..}, CreateSpace{..},
               SetSetting{..}, GrantAgent{..}, RevokeAgent{..}, ... }   // exact list in crates, not here

// Private to yaac-core. Built by each ingress transport after it authenticates the caller.
enum Principal { Ui{view: ViewId}, Agent{client: ClientId}, Import{job: JobId, on_behalf_of: Box<Principal>}, System }
```

**Authority is never taken from the request.** The wire format carries no actor field. Each ingress builds the `Principal` itself: the `app://ui` scheme handler from a UI view id the core created, the socket server from an enrolled token, an import job from the principal that started it. A table in `yaac-core` maps each `Principal` kind to the commands it may issue. `Agent` principals cannot issue `GrantAgent`, `RevokeAgent`, `SetSetting`, enrollment approval or anything that edits policy; only `Ui` principals can, and only from a UI view. An import job keeps the starting principal and its destination restrictions; it does not acquire authority of its own.

The actor handles a request in four steps: **authorize** (principal, grants), **validate**, **apply** (mutate state, produce engine effects and store writes), **emit** events.

**Durable state and runtime state are separate.** The store holds the desired state (nodes, Spaces, settings). Runtime facts (which tabs currently have a browser, which navigation generation a view is on) are never persisted. Commit-before-effect is not enough on its own: a crash between the store commit and the engine effect leaves saved state ahead of the engine. So on start the core **reconciles**: every restored tab comes up `suspended`, the selected tab is loaded, and nothing replays a page action. Effects that fail (a browser that did not create) become events that the actor handles.

Properties this buys:

- The web UI, the CLI and MCP cannot disagree, because they call the same commands.
- Agent grants (section 11) are enforced at one choke point. There is no route to state that skips it. This answers the Aside finding that its `repl` path ran outside the Settings rules (`docs/research/06-aside-observed.md`, section 11).
- Every command can be audited, replayed in tests and (later) undone.

**Events.** The core publishes `Event { topic, rev }` after a change. Events carry a revision number, not state. A client pulls changes with `Query::Since(rev)`, filtered to what that principal may see. Pushes stay small and coalescable. Revision events alone do not fix the title mismatch seen in the prototype (a new URL paired with the old title), because the actor can publish a revision between the address callback and the title callback. So the actor ties title and URL to the engine's navigation generation: a navigation start clears the stale title, and a stale `TitleChanged` (older `nav_gen`) is dropped. `Since` keeps a bounded window of revisions; a client that falls outside it gets a full resync.

**Undo.** Commands that remove or move nodes record an inverse in a bounded history. Closed tabs stay recoverable (see archive in section 7).

## 7. Data model

Arc's sidebar data, as recorded in the project brief from an earlier session (not re-verified in this repository), stores tabs, folders and split views as nodes in one tree with `id`, `parentID`, `childrenIds`, `title`, `createdAt`, `isUnread`, `originatingDevice` and a typed `data` payload. Spaces carry an id, a profile and container id lists. We use the same *shape* (a typed node tree). This document specifies our own schema and copies nothing else.

Concepts:

- **Profile**: an isolated browsing identity. One CEF request context and cache path (cookies, storage). Kinds: `default`, `user`, `agent`. Design; isolation between contexts is **open** (A7) and gates both profiles and agent grants.
- **Space**: a named, themed group of tabs belonging to one profile. Many Spaces can share a `default` or `user` profile; those Spaces are organizational boundaries only and share cookies and sessions. A Space has three containers: `pinned`, `today` (unpinned) and `archive`.
- **Agent Spaces**: a Space an agent can be granted must belong to an `agent` profile that no other Space uses. The core rejects a grant on any other Space, and in particular on the `default` profile. Giving an agent a logged-in identity means signing in inside that agent profile. This keeps the per-Space cookie toggle meaningful, because a Space's cookies are the profile's cookies.
- **Favorites**: a per-profile container above the Spaces, shared by all Spaces of that profile.
- **Node**: an element in a container's tree. Kinds: `tab`, `folder`, `split` (a group of two to four tabs shown together), `note` later. Folders and splits can nest in folders.
- **Tab lifecycle**: persisted state is `suspended` (no browser; url, title, favicon and scroll kept). `live` (has a browser) is runtime-only and never stored. A tab is archived by moving its node to the `archive` container, which is the single record of that fact.
- **Auto-archive**: unpinned tabs not touched for a user-set period (default 12 hours; 24 hours, 7 days, 30 days or off) are archived by a core timer. Pinned tabs never auto-archive.
- **Boost**: a per-site bundle of CSS and JS the user writes, created with an element picker. Stored per profile, injected by the engine layer on matching origins.

SQLite (via `rusqlite`, bundled, WAL, one writer). Sketch, not final:

```sql
CREATE TABLE profile (id TEXT PRIMARY KEY, kind TEXT CHECK (kind IN ('default','user','agent')),
                      name TEXT, cache_dir TEXT, created_at INTEGER);
CREATE TABLE space   (id TEXT PRIMARY KEY, profile_id TEXT REFERENCES profile(id),
                      name TEXT, theme TEXT, position TEXT);        -- agent grantability is derived: profile.kind = 'agent'
CREATE TABLE container (id TEXT PRIMARY KEY, profile_id TEXT REFERENCES profile(id),
                        space_id TEXT REFERENCES space(id),        -- NULL for favorites
                        kind TEXT CHECK (kind IN ('favorites','pinned','today','archive')));
CREATE TABLE node (
  id TEXT PRIMARY KEY,                 -- UUIDv7
  container_id TEXT REFERENCES container(id),   -- the single ownership root: profile and Space are derived from it
  parent_id TEXT REFERENCES node(id),  -- NULL = directly under the container
  position TEXT,                       -- fractional index, unique with parent_id; ties broken by id
  kind TEXT CHECK (kind IN ('tab','folder','split')),
  title TEXT, url TEXT, favicon BLOB,
  state TEXT DEFAULT 'suspended',      -- persisted state only
  is_unread INTEGER DEFAULT 0,
  created_at INTEGER, last_active_at INTEGER,
  updated_at INTEGER, origin_device TEXT, rev INTEGER   -- reserved for a later sync; not a sync design
);
CREATE TABLE visit   (id INTEGER PRIMARY KEY, profile_id TEXT, nav_id TEXT, url TEXT, title TEXT, ts INTEGER);
CREATE TABLE boost   (id TEXT PRIMARY KEY, profile_id TEXT, origin TEXT, css TEXT, js TEXT, enabled INTEGER);
CREATE TABLE setting (scope TEXT, scope_id TEXT, key TEXT, value TEXT, PRIMARY KEY (scope, scope_id, key));
CREATE TABLE agent_client(id TEXT PRIMARY KEY, name TEXT, token_hash TEXT, created_at INTEGER, revoked_at INTEGER);
CREATE TABLE agent_grant (client_id TEXT, space_id TEXT, scopes TEXT, origins TEXT, generation INTEGER, expires_at INTEGER);
CREATE TABLE audit_log (id INTEGER PRIMARY KEY, request_id TEXT, phase TEXT,   -- started | denied | cancelled | completed | interrupted
                        ts INTEGER, client_id TEXT, space_id TEXT, tool TEXT,
                        scope TEXT, origin TEXT, result TEXT, duration_ms INTEGER);
```

Tree invariants (enforced in `yaac-model`, checked again by the store): a node's profile and Space are derived from its container, so a node cannot disagree with its owner. No cycles. A `split` has two to four `tab` children and nothing else. Moving a node moves its subtree in one transaction and touches only the moved root's `parent_id` and `position`. A move across containers in the same profile is allowed and recreates no browsers. A move across profiles is **not allowed in v1**, because a live browser keeps the old profile's session; it would need a close-and-recreate. Sibling order uses fractional indexing so an ordinary move touches one row. Keys are base-62 strings, ties are broken by `id`, and the siblings of a parent are renumbered when a key grows past 32 characters. Deletion tombstones and concurrent-insert conflicts belong to the sync design, which is deferred.

Notes: `rev` and `origin_device` are reserved columns only. Migrations are numbered SQL files applied in one transaction and recorded in `PRAGMA user_version`. Browsing history (`visit`) is our own table, fed from engine events and keyed by navigation id, because CEF exposes no history API for the omnibox. Reloads and redirects update one visit; the exact dedup rules are a skeleton-time decision. The audit log is append-only as an application guarantee only; a same-user process can still edit the file. Page content is never stored. The audit log never stores typed values or page text.

## 8. Web UI and the IPC protocol

The UI is TypeScript, built to static files and served from the custom scheme `app://ui/`, registered in the browser and helper processes (observed in the prototype). It is a set of pages: sidebar, command bar, settings, confirmations, agent log.

**Transport (design, validated in part by the prototype):**

- UI to core: `fetch('app://ui/api/v1/rpc', { method: 'POST', body: JSON })`. The scheme handler decodes a `Request`, sends it to the actor and returns the `Response`. Observed in the prototype: the sidebar's `fetch()` calls (GET) reached the handler, and Rust called `load_url` on the content frame. POST bodies are untested.
- Core to UI: events are pushed with `execute_java_script('window.yaacEvent({...})')` and carry only `{topic, rev}`. The UI then pulls with `Query::Since(rev)`.
- The wire format is JSON, versioned with `v` in every request. Rust types in `yaac-api` are the source of truth. The TypeScript types are generated from them.
- `CefMessageRouter` is bound in cef-rs but unexercised in its repo (sourced). It is the fallback if the scheme path hits a limit. The UI bridge sits behind a `UiBridge` trait, so the choice stays local to `yaac-cef`.

**Authorization of UI calls.** The scheme handler authenticates the caller by the browser id of a view the core created for the UI, not by the frame's URL. A call from any other browser is rejected. The handler fails closed: if it cannot attribute a request to a UI browser, it refuses. Content browsers cannot reach UI pages: `on_before_browse` refuses `app://` navigations in content browsers and popups, and content views get no bridge. UI pages render untrusted text (page titles, URLs), so they ship with a strict CSP and escape that text. Observed in the prototype: a `fetch()` GET from the content view to a sidebar-only endpoint returned 403. That test used a local `app://` page as content, and it used GET only. POST with a JSON body, remote `https` pages, embedded frames and navigation races are untested and **open** (experiment E-IPC-1: from a remote page, attempt `fetch` and `XMLHttpRequest` to `app://ui/api/v1/rpc`, with and without CORS, from the page and from a child frame, and attempt to navigate a content browser to `app://ui`).

## 9. Windows, views and tab lifecycle

- One `CefWindow` per browser window. A root panel with a horizontal box layout holds the sidebar view (fixed width, collapsible) and the content area (observed).
- Each `live` tab is a CEF browser in a `BrowserView`. Only the selected tab's view is visible. Switching tabs swaps the visible view. The core keeps the node-to-browser map.
- Suspending a tab destroys its browser and keeps the node. Restoring creates a new browser and loads the saved url. Memory limits and the suspend policy are tuning work after the skeleton.
- Split view, Peek and Little Arc are later features. The model supports them: `split` is a node kind, and Peek and Little Arc are extra windows or overlay views created through `EngineHost`.
- Popups: `on_before_popup` must answer synchronously on the CEF thread without waiting for the actor. It denies the popup and emits `PopupRequested{opener, url, user_gesture}`. The core then decides and, if allowed, opens a new tab node in the same Space through a command (design; the prototype denies popups and does nothing more). The cost is that the new tab has no `window.opener`, which breaks some sign-in flows. How to handle those is **open**.
- Swapping the visible content view when the user switches tabs was not tested in the prototype (it had one content view). It is part of the M1 acceptance criteria (section 15).

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

**Verification** (test, not assertion): a CI and release check launches the app with an empty profile, idles for 60 seconds, and records every request Chromium makes (a net log, `--log-net-log`) plus a packet or DNS capture over the full period on a host where capture is permitted. A single `lsof` snapshot misses short-lived connections, so it is only a supplement. The expected set is empty. The first run of this test is the next step on the Google-connection finding.

Agent state, grants and the audit log stay local and are never uploaded.

Extensions: uBlock Origin MV2 as a built-in component with a patch that keeps the MV2 gate open (recommended in `docs/research/03-extensions-mv2.md`, sourced, not run). Risks: Chromium may remove the MV2 path; CEF has had no public extension API since 2024; an update path for the bundled extension and its lists must not depend on the Web Store. External extension providers are disabled in our build: on Omarchy a system file under `/usr/share/chromium/extensions/` made stock CEF install an extension (observed, E2). A Rust-side blocker (`adblock-rust`, MPL-2.0) is the fallback, not v1.

## 11. Agent interface

Decisions in `docs/decisions.md` (2026-10-23): no built-in AI; agents get a CLI and an MCP server; token efficiency and safe defaults are requirements; `evaluate` ships behind confirmation; agents act only in Spaces created or marked for agents; confirmation only for sensitive actions; cookies off by default with a per-Space toggle; schema public and versioned; Windows transport deferred.

**Shape.** `yaac-agent` runs inside the app process and exposes the core API to external clients over a Unix domain socket. `yaac <cmd>` and `yaac mcp` are thin clients of that socket. Requests arrive as `Actor::Cli` or `Actor::Mcp` and pass through the same authorize step as the UI.

**Transport and enrollment (R1, R2).**
- Socket in a per-user runtime directory, mode 0700, socket 0600. The server checks the peer uid on each connection (`getpeereid` on macOS, `SO_PEERCRED` on Linux). No TCP listener, not even on loopback. Aside's daemon listens on a loopback TCP port whose `/` and `/health` routes answered without credentials; whether its other routes need a token is unknown (observed in `docs/research/06-aside-observed.md`, section 5). Any local process can reach such a port, which is the design we avoid.
- A client enrolls once. The user approves it in the app (a confirmation page rendered by the trusted UI). The client receives a grant token stored in a 0600 file. Tokens are revocable and short-lived, and renew only while the grant stands.
- A same-user process holding a token can drive the granted Space. Socket permissions do not stop same-user malware. This limit is stated, not hidden.

**Grants and scopes (R3, R4, R13).** A grant is (client, Space, scopes, expiry). A Space maps to one profile. The default agent Space is dedicated. A Space with logged-in state is reachable only through an explicit grant on a Space marked `agent_enabled`. Never the default profile. Scopes: `read` (list, snapshot, text, screenshot, console), `act` (navigate, click, fill, type, press, select), `privileged` (`evaluate`, `download`), and `cookies` (a separate scope, off by default, enabled per Space in settings; cookie values never go to MCP output). A grant also carries `origins`: `any` (the default for a dedicated agent profile) or an allow-list of origins and schemes. Only `http` and `https` are navigable by agents; `file:`, `app:`, `chrome:` and DevTools URLs are refused. With an allow-list, navigation, redirects and subresource egress are checked below the tool handlers, not inside them. This is what stops an injected instruction from reading private content and sending it to an attacker URL; confirmation alone does not, so the allow-list option exists for Spaces that hold logged-in sessions. Whether `any` should be the default, and how the sign-in case is handled, is an owner decision (section 15).

**Confirmation (R5).** For ordinary actions, confirmation is required only for sensitive ones (the owner's decision): form fields for payment or credentials, account-settings pages, and origins on a user-managed list. `evaluate` and `download` ask each time, as decided with the `evaluate` answer. Confirmation is rendered by the trusted web UI, never by page content, and enforced in the core. The detector is conservative; its rules (field types and autocomplete tokens, URL patterns for account and payment pages, the origin list) are defined here and in `docs/agent-api.md` when the schema is written. A first action on a new origin does not prompt on its own.

**Execution protocol.** Authorization happens in the actor, but execution does not. The actor checks the principal and grant, then issues a **ticket** bound to (principal, grant generation, tab and navigation generation, origin, exact action) and hands it to the executor, which runs off the actor. A confirmation is consumed once and is bound to the same tuple. The executor revalidates the ticket at dispatch, so a tab that navigated, changed Space or lost its grant while approval was pending fails closed. The kill switch bumps the grant generation, which invalidates every ticket and cancels queued effects; what already-running page JavaScript does after that is **open**. `yaac-agent` never calls `PageDriver` directly. Downloads need the same provenance: a download started by a click or keypress in an agent-driven tab (`DownloadRequested`) is treated as the `download` privilege and is denied unless that is confirmed.

**UI views are not agent targets.** A `PageDriver` is only handed out for content views in a granted Space. DevTools targets for UI views are never exposed, so an agent cannot drive its own confirmation page. Whether `Target.*` could reach UI views by accident (A1) must be ruled out by test.

**Tools (v1 target: 8 plus 2 privileged).** `tabs` (list, open, close, select, within the granted Space), `navigate` (url, back, forward, reload, optional wait), `snapshot`, `act`, `text`, `screenshot`, `console`, `wait`; privileged: `evaluate`, `download`. Descriptions are short and factual, with no policy prose, because policy is enforced by the core (Aside's four-tool schema is about 8.8 KB, mostly one tool's prose; observed). The schema is public and versioned from the first release.

**Page representation and token cost.** `snapshot` returns an indented accessibility tree with short refs (`e1`, `e2`), a `selector` scope, and a diff after actions when the previous snapshot used the same options. Defaults: output capped at 16 KiB, text inside a node capped with an explicit `[+N chars]` marker, and a `truncated: true` result with a cursor or a suggested narrower scope. Refs distinguish "not found" from "stale". Screenshots return a file path in the Space's artifact folder and are sent as an image only on request, resized. Measured baseline for the same pages (raw HTML against Aside's snapshot): example.com 577 B against 270 B, a GitHub repo page 245,228 B against 6,595 B, Wikipedia 415,391 B against the 16 KiB cap. Our targets are the same order of size. We verify with experiment A8 on our own output.

**Page output is untrusted (R9).** Text from pages is wrapped in a labeled boundary and marked `untrusted: true`. This is a cue for the model, not a defense. The defense is architectural: grants, origin scope and confirmations in the core.

**Visibility and control (R6 to R8).** Tabs an agent drives appear in a group for that Space. The Space shows "agent connected" while a grant is live. An append-only audit log records time, client, Space, tool, scope, origin, result and duration, never page text or typed values. A per-Space kill switch revokes every grant and cancels in-flight commands (`yaac agent stop` does the same from the CLI). Click-to-stop latency is not measured (**open**).

**Limits (R12).** Maximum output size per response, a per-client rate limit, and a concurrency cap per Space. Errors carry no stack traces or internal paths (Aside's do; observed).

**CLI behavior.** Unknown subcommands are errors. A failed script exits non-zero. Leaf `--help` prints leaf help. Values go to stdout and status to stderr, with no ANSI when piped. These are the defects observed in Aside's CLI.

**What the cookie toggle authorizes.** It enables the `cookies` scope tools for that Space and nothing else. Ambient authenticated browsing (the agent using the profile's sessions by loading pages) follows from granting a Space that holds sessions, and arbitrary confirmed `evaluate` is a separate privilege. These are three different capabilities and the UI describes them separately.

**Memory-type tools** that read personal data stay out of the browser server (Aside puts `memory_search` in the same MCP server; observed).

## 12. Settings

Settings are typed keys in `yaac-api` (name, type, default, scope: global, profile or Space), stored in the `setting` table and edited in the web UI. Examples: auto-archive period, sidebar width, cookies-for-agents per Space, sensitive-origin list, filter list sources, agent server on or off (default off). A setting that changes privacy behavior needs an entry in `docs/network-calls.md` or the decision log.

## 13. Importers

`yaac-import` reads from the source's files on disk, read-only, and emits `Command`s as `Actor::Importer`. The Arc importer reads sidebar data (spaces, pinned and today tabs, folders). Chromium-family importers read bookmarks, history and, with consent, passwords through the platform `SecretStore`. Firefox and Safari follow. The existing `ardia` fork's importers are a reference for what exists on disk; verify the license of each file before reusing code (the project brief says its own files are Apache-2.0 and the app GPL-3.0; that claim has no source in this repository, so check each file).

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

**M1: macOS walking skeleton.** Workspace, `yaac-model`, `yaac-store` (including profiles, Spaces and containers), `yaac-core` with FakeEngine tests, `yaac-cef` on cef-rs, a window with the sidebar and a tab tree, open, close and switch, persisted across restarts. On a stock prebuilt CEF run with `--disable-extensions` (the stock build otherwise loads external extensions; observed in E2). Acceptance includes three things the prototype never tested: swapping the visible content view on a tab switch, E-IPC-1 (a remote page cannot reach `app://ui/api`), and A7 (two request contexts isolate cookies and storage).

**M2: Linux skeleton on Omarchy.** Same skeleton on Wayland, `yaac-platform` Linux implementations, Arch package. Acceptance requires observed painting and input on Hyprland, not only a started process.

**M3: agent interface alpha.** `yaac-agent`, `yaac` CLI and `yaac mcp`, `snapshot` and `act`, grants on `agent` profiles, tickets and confirmation, the audit log. Depends on A7 and on minimal profile and Space ownership from M1. Ahead of the command bar because it is needed daily. The richer Spaces and profiles UI waits for M4.

**M4:** command bar, Spaces and profiles UI, auto-archive, the Arc importer, Boosts, split view.

**M5:** the de-Googled engine build (Helium then CEF), the cef-rs and CEF version decision, the idle-network test, the extension decision, and the privacy audit.

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
| 11 | Which sign-in flows break without `window.opener`, and what do we do about them? | Test Google and GitHub sign-in popups in M1 |
| 12 | What happens to running page JavaScript after the kill switch? | Measure in M3 |
| 13 | Does a `Target.*` or DevTools path ever reach UI views? | A1, with UI views in the target list |

**Owner decisions needed** (recommendation in brackets): (a) Should `evaluate` and `download` confirm every time, with ordinary actions confirming only when sensitive? [yes, as written in section 11]. (b) Should the default grant allow `any` origin in a dedicated agent profile, or start with an allow-list? [`any` in an empty agent profile, allow-list recommended once the profile holds sign-ins]. (c) ts-rs or specta for TypeScript types? [ts-rs for M1].

## 16. Decisions to record after review

1. One typed command API serves the UI, CLI and MCP; the actor model and single writer.
2. UI to core over the `app://ui` scheme with revision-only events, behind a `UiBridge` trait.
3. The node-tree schema and fractional ordering; reserved sync columns.
4. Helium patches first, then CEF's (already recorded from E1).
5. The `Platform` trait list and the rule that the core never names OS or CEF types.
6. Agent interface scopes, transport and the limits listed in section 11.
7. Authority comes from the ingress, never from the request; agents cannot edit policy.
8. A grantable Space owns a dedicated `agent` profile; no cross-profile moves in v1.
9. Split-phase engine interface: the core allocates ids and results come back as events.
