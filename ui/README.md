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
- `sidebar.html`, `vite.config.ts` and `csp.mjs` are the entry page, the build config and the strict Content-Security-Policy. The CSP is injected into the built HTML only, because the dev server needs inline scripts and a websocket, which the policy blocks.

## Tests

The tests cover nested rendering, live and suspended states, unread markers and expansion. They check that markup in a title stays text. They check that every action sends the exact `Command` JSON, and the expected values are typed as `Command`, so drift from Rust fails `tsc`. They cover event coalescing, stale-response handling and retry without a loop. They cover keyboard navigation, rename and close. They check drag and drop placements, including invalid drops. They check the address allow-list, which rejects `javascript:`, `file:`, `data:` and `app:` addresses. They check the HTTP bridge request shape and event delivery. A full round trip runs against `MockBridge`.

## Accessibility

The tree follows the ARIA tree pattern. It uses `role="tree"`, `treeitem` and `group`, with `aria-level`, `aria-expanded`, `aria-selected` and a roving tabindex.

- Arrow keys move focus. Right and Left expand or collapse a folder, or move to its child or parent.
- Enter or Space selects a tab or toggles a folder.
- Delete or Backspace closes a tab.
- F2 starts a rename.
- Alt+Up and Alt+Down move a node among its siblings.
