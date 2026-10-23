# yaac sidebar UI

The sidebar is a Preact page. The browser loads it as `app://ui/sidebar.html` into a fixed-width
CEF view about 240 px wide. It shows the active Space, an address field for the selected tab, and
the `pinned` and `today` containers as a tree.

The page makes no network requests. Every asset is bundled and referenced by a relative path.
There are no CDNs, web fonts or analytics.

## Commands

Run these in `ui/` with pnpm and Node 24.

| Command | What it does |
|---|---|
| `pnpm install --frozen-lockfile` | Installs from `pnpm-lock.yaml`. |
| `pnpm dev` | Starts the Vite dev server at `http://127.0.0.1:5173/sidebar.html`, using the in-memory mock core. |
| `pnpm test` | Runs the Vitest suite with jsdom and Testing Library. |
| `pnpm typecheck` | Runs `tsc --noEmit`. It fails when `src/generated/` drifts from the Rust types. |
| `pnpm lint` | Runs ESLint with a flat config. It applies the recommended ESLint and typescript-eslint rules and bans HTML injection sinks in `src/`. |
| `pnpm build` | Runs the typecheck, then `vite build` into `dist/`, then `scripts/check-dist.mjs`. |
| `pnpm gen` | Regenerates `src/generated/` from the repo root with `cargo test -p yaac-api --features ts`. |

Do not edit `src/generated/` by hand.

## Which core the page talks to

The page uses one of two bridges.

- Inside the host, on the `app:` scheme, the page uses `HttpBridge`. It sends `POST app://ui/api/v1/rpc` with a versioned `Request`, and it receives events through `window.yaacEvent`.
- Under `pnpm dev`, outside the host, the page uses `MockBridge`. That is an in-memory tree which implements every command in the generated `Command` union. It is a dev-only chunk and is not part of the production build.

## Layout

- `src/wire.ts` is the only import point for the generated types. It also holds the endpoint constants.
- `src/bridge/` holds the `Bridge` interface, `HttpBridge` and `MockBridge`.
- `src/state/sidebar.ts` holds the `Sidebar` controller. It owns sync and every action, and it builds the exact `Command` for each one.
- `src/state/store.ts` is a small observable store.
- `src/model/` holds pure helpers for tree lookup, visible rows, drop placement, titles and the address allow-list.
- `src/ui/` holds the Preact components. Titles and URLs render as text only.
- `src/ui/drag.ts` holds the drag MIME type. It carries a random token generated at start-up, kept in memory only. A drag counts as internal only when its type list contains that exact type.
- `sidebar.html`, `vite.config.ts` and `csp.mjs` are the entry page, the build config and the strict Content-Security-Policy. The CSP is injected into the built HTML only, because the dev server needs inline scripts and a websocket, which the policy blocks.

## Tests

The tests cover nested rendering, live and suspended states, unread markers and expansion. They check that markup in a title stays text. They check that every action sends the exact `Command` JSON, and the expected values are typed as `Command`, so drift from Rust fails `tsc`. They cover event coalescing, stale-response handling, timeouts, backoff and recovery, using fake timers. They cover keyboard navigation, rename and close. They check drag and drop placements, including invalid drops. They check the address allow-list, which rejects `javascript:`, `file:`, `data:` and `app:` addresses. They check the HTTP bridge request shape and event delivery. A full round trip runs against `MockBridge`.

## Accessibility

The tree follows the ARIA tree pattern. It uses `role="tree"`, `treeitem` and `group`, with `aria-level`, `aria-expanded`, `aria-selected` and a roving tabindex.

- Arrow keys move focus. Right and Left expand or collapse a folder, or move to its child or parent.
- Enter or Space selects a tab or toggles a folder.
- Delete or Backspace closes a tab.
- F2 starts a rename.

### Keyboard moves

Each move sends one `move_node` with an exact placement. Focus stays on the moved row.

| Shortcut | Move |
|---|---|
| Alt+Shift+Up or Alt+Shift+Down | Reorder among siblings |
| Alt+Shift+Left | Move out of the folder, to just after it |
| Alt+Shift+Right | Move into the folder directly above, at its end. The folder expands. |
| Alt+Shift+P | Move to the end of Pinned |
| Alt+Shift+T | Move to the end of Today |

Plain Alt chords are not bound. Letter shortcuts use the physical key, because Alt changes the character on macOS.

### Spaces

The Space list shows every Space. Clicking one sends `select_space`. The active Space has `aria-current`. New Space takes a name inline, creates the Space, then switches to it. A new folder starts an inline rename as soon as the core returns its id.

## Failure handling

Every request has a 10 s timeout. Failures fall into two classes.

- Transient: a timeout, a transport failure, an HTTP 5xx status, or an `internal`, `not_found` or `conflict` reply. The status line says the page is reconnecting. A failed pull retries with backoff (250 ms, doubling, capped at 5 s) until a snapshot arrives. A new event cancels the backoff and pulls at once. The tree keeps the last known state.
- Permanent: an `unsupported_version`, `forbidden` or `bad_request` reply, a 403 or other 4xx status, or a reply the validator rejects. The page shows a refusal message with a Retry button. It stops retrying, and events do not revive it. Only Retry does.

A command that times out shows an error and pulls a fresh snapshot. A command's own error (for example `bad_request` on a rename) shows a notice and does not block the tree. A notice stays until it is dismissed or the next command succeeds.

`stop()` ends all pending work. It clears the subscription, the scheduled pulls and the retry timer. A reply that arrives after `stop()` is dropped, and nothing is sent until `start()` is called again. `start()` always resyncs.

## Addresses

Addresses typed without a scheme default to http for loopback hosts (`localhost`, `127.0.0.0/8`, `::1` in any form, and IPv4-mapped 127.x) and for IPv6 literals. Every other host defaults to https. The 2048-character cap applies to the final URL after percent-encoding.
