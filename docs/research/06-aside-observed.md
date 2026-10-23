# 06. Aside, observed from outside (partial run)

Date: 2026-10-23. Status: PARTIAL. The Aside CLI is not installed on this Mac, so `aside mcp`, `tools/list`, the action tools, and the snapshot measurements could not be run. Everything below was observed without the CLI or MCP, or was read from the app bundle.

Scope rules followed: clean room. No disassembly or decompilation. No reads of implementation code, prompts, models, or assets. No reads of any file under `~/.aside/` (names, modes and sizes only). No Aside AI features, no sign-in, no approvals, no tabs opened or changed. No install of the CLI. No sudo.

Labels: VERIFIED = observed in this run (command output, a binary's import/export/string names, or a plist/entitlement value). INFERRED = a reasoned conclusion from VERIFIED facts. Items from the public docs are marked DOC (carried over from note 05, not re-checked here).

Raw captures (small text files): `docs/research/06-aside-observed/runtime-probes.txt` and `docs/research/06-aside-observed/static-inventory.txt`. Larger static extractions were written to `$TMPDIR/re/aside` and are not in the repo.

## Summary

- Aside is installed at `/Applications/Aside.app`, version 1.0.1022.1. The app contains a Node-based local daemon, `aside-daemon` (bundle id `at.studio.AsideBrowser.daemon`, version 1.26.1023). It listens on TCP `127.0.0.1:21420`. VERIFIED.
- The daemon rejects requests with a foreign `Origin` or `Host` header with HTTP 403. A plain `GET /` with no headers returns HTTP 200 and a 231-byte JSON body. No credential was sent. VERIFIED. The body was not read.
- The only observed TCP client of the daemon is the browser's network-service helper process. VERIFIED. The CLI and `aside mcp` were not observed (they are not installed).
- The CLI is not on PATH and is not at any documented location. VERIFIED. The daemon binary carries the install-target strings `~/.local/bin` and `~/.aside/bin`. VERIFIED as strings; INFERRED as the install targets.
- Tool count, tool schemas, snapshot size, permission behavior, and action-tool shapes are UNKNOWN for this run. They need the CLI.

## 0. Blocked items and why

- CLI not installed. Doc says `curl -fsSL https://releases.aside.com/install.sh | bash` installs it (DOC). Not run: it executes a remote script and writes into `~/.local/bin` or `~/.aside/bin`, which is outside the worktree and outside the scope of this run.
- `aside login` needs approval in Aside Settings > Account > Devices (DOC). Not run; the rules forbid approving anything on the user's account.
- Without the CLI, there is no `aside mcp` process to drive over stdio. The `tools/list` call (experiment A6) and the MCP schema capture are blocked.
- Action tools and permission prompts need a tool call through a client. Blocked for the same reason. No browser tab was created by hand either, so no consent dialog was observed.
- Snapshot size in bytes and tokens for Aside (experiment A8) is blocked. Baseline page sizes are recorded in section 6.

## 1. Install facts

VERIFIED unless marked otherwise.

- App bundle: `/Applications/Aside.app`. Bundle id `at.studio.AsideBrowser`. `CFBundleShortVersionString` 1.0.1022.1, `CFBundleVersion` 1022.1. Minimum macOS 13.0. Arch arm64 only.
- Framework: `Contents/Frameworks/Aside Framework.framework`, version 1.0.1022.1 (the same version as the app). It contains helper apps (GPU, Renderer, Alerts, Aperitif variants, Updater) and `Libraries/` with `AsideAgentManager`, `AsideDaemon`, `AsidePasswordManager`, and `libaperitif.dylib`. The names are from the bundle layout.
- Daemon: `Libraries/AsideDaemon/mac-arm64/Aside Daemon.app`, bundle id `at.studio.AsideBrowser.daemon`, version 1.26.1023. This version differs from the app version. The daemon is a separate component with its own version.
- Chromium: the framework binary contains the user-agent constant `Chrome/154.0.8037.100`. INFERRED: the Chromium major version is 154. The probe that would confirm it on a live page was not run (no tab). note 05 pins CEF 154.5.0, which matches.
- Code signing: Developer ID Application, team `8CPD4K4TBB`, hardened runtime flags `kill, restrict, library-validation, runtime`. Notarization ticket stapled. VERIFIED (`codesign -dv --verbose=2`).
- Entitlements, main app: web-browser public-key credentials, audio input, bluetooth, camera, print, USB, location, photos library, and keychain groups for cryptography, device trust, secure payment confirmation, unexportable keys, and WebAuthn. VERIFIED.
- Entitlements, daemon: `allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`, and a keychain group for cryptography. VERIFIED. `disable-library-validation` on a process that listens on a TCP port is a wider attack surface than needed. This is a concern for us, not a finding about Aside's security.
- Linked libraries. Main `Aside`: `libSystem` only. Daemon: CoreFoundation, Security, libc++, libSystem. VERIFIED (`otool -L`).
- Exports/imports: main binary has one export (`_main`) and 40 undefined imports (`nm -gU`, `nm -u`). Daemon: about 38,000 exported symbol lines, most from the bundled JavaScript runtime. VERIFIED counts only.
- The daemon's strings include Node.js runtime flag names and a native addon `aside-native.node`. INFERRED: the daemon is a Node.js process with a native addon.
- Update path: strings include `ASIDE_CLI_UPDATE_CHECK`, `ASIDE_CLI_RELEASE_FETCH_TIMEOUT_MS`, and `ASIDE_CLI_INSTALLER_PATH`. VERIFIED as names. DOC says `aside --update` updates the CLI.

## 2. CLI

Not observed. The binary is absent.

- PATH: `which aside` returns nothing. The directories `~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin`, `~/.aside/bin`, `~/.aside/cli` do not contain an `aside` file. VERIFIED.
- Install-target strings in the daemon: `ASIDE_CLI_INSTALL_DIR`, `ASIDE_CLI_CANDIDATE_PATHS`, `ASIDE_CLI_EXPECTED_PUBLISHER`, and the path strings `~/.local/bin` and `~/.aside/bin`. VERIFIED as strings. INFERRED: the installer or the app's CLI-setup step puts the CLI in one of these directories and adds it to PATH. How PATH is set up was not checked; the shell rc files were not edited, and no PATH line for Aside was found in `~/.zshrc`, `~/.zprofile`, or `~/.zshenv` (grep for "aside", VERIFIED).
- Command tree: DOC only (note 05, section 1.1). Not re-verified this run.
  - `aside "<prompt>"`: starts an agent task (paid agent path; not used here).
  - `aside --session <id> "<prompt>"`: continues a session (agent path; not used).
  - `aside account list | status [id] | use <id>`, and `--account <id>` on `aside` and `aside exec`.
  - `aside exec` with `-m` for a model. Agent path; not used.
  - `aside login --email <addr>` with `ASIDE_CLI_AUTH_PATH`. Not run.
  - `aside mcp`: MCP server. Not run.
  - `aside repl "<js>"`: JavaScript against a page API. Not run.
  - `aside --update`.
- Output formats, exit codes, and `--help` text: not observed.
- The REPL: not observed.
- Sign-in state: `~/.aside/cli/` does not exist, so no CLI credential file exists. VERIFIED by name. This means the CLI has not been logged in on this Mac.

`ASIDE_CLI_AUTH_PATH` (DOC) was not found in the daemon's strings. The search was limited to one binary, so this is not a finding.

## 3. MCP

Not run. The `aside mcp` process does not exist without the CLI.

Static cross-check only (VERIFIED as strings; the role was not determined):

- Protocol method names found in the daemon strings: `initialize`, `tools/list`, `tools/call`, `notifications/initialized`, `resources/list`. `prompts/list` was not found.
- MCP-related identifiers: `McpManager`, `McpClient`, `McpStdioConnectionError`, `McpHttpError`, `McpOAuthProvider`, `McpOAuthAuthorizationRequiredError`, `mcpServers`, `mcp__` prefix, `mcp-credential-cleanup` (file name).
- INFERRED: the daemon contains an MCP client that connects to external MCP servers (the `mcpServers` config key). The strings do not show whether the daemon also contains an MCP server that `aside mcp` would expose. Both roles could be in the same process. This is the open question for the MCP section.
- No `serverInfo` or `protocolVersion` capture was taken. The tool list is UNKNOWN.

Tool table: UNKNOWN (no `tools/list`). note 05's secondary-source claim of a single `repl` tool is not confirmed or refuted.

## 4. Architecture

VERIFIED, with the INFERRED parts marked.

```
Aside main  (pid 13436, Chromium browser process)
  |-- TCP 127.0.0.1:45103 (LISTEN). Plain GET returns 403, text/html. Identity not established.
  |-- unix SingletonSocket in $TMPDIR/at.studio.AsideBrowser.<random>/ (browser singleton)
  |
  +-- Aside Helper, NetworkService (pid 13445, child of 13436)
          |  TCP client, 7 connections to 127.0.0.1:21420
          v
      Aside Daemon (pid 13450, Node.js based, child of 13436)
          |-- TCP 127.0.0.1:21420 (LISTEN). Host/Origin check. Plain GET returns 200 JSON.
          |-- one outbound TLS connection to a Cloudflare address on port 443
          |-- opens ~/.aside/logs/daemon-*.log (mode 600), ~/.aside/u/0/state.db (SQLite), and the password-manager vault
          `-- has a child process aside-memory (pid 13481) under ~/.aside/memory-runtimes/ (purpose INFERRED from the path)

CLI `aside` and `aside mcp`: not installed. Path to the daemon: UNKNOWN.
```

Observations behind the diagram:

- The daemon and the main process both listen on loopback TCP. No Unix socket was found for the daemon. VERIFIED (`lsof -U` for pid 13450 shows only an fd to a peer address, not a listening socket path).
- Seven accepted daemon connections, all from pid 13445. VERIFIED (`lsof -iTCP:<port>` for each peer). The daemon's peer is the browser network helper, not a CLI. The purpose of this link is INFERRED from the process role.
- Host and Origin checks: a request with `Host: evil.test` and a request with `Origin: https://example.com` each returned 403. VERIFIED (headers only, via curl). These two tests do not test a real browser's CORS behavior. A browser request was not sent.
- Authentication: a plain `GET /` with no `Origin` and no `Authorization` returned 200. VERIFIED. The 231-byte body was not read. Whether it is an error envelope or a status document is UNKNOWN. An unauthenticated 200 on loopback means any local process can reach the port. VERIFIED for the reachability; INFERRED that other routes may need a token.
- Token: no token file was found by name. `~/.aside/u/0/credentials.json` (mode 600, 2 bytes) exists by name, and `~/.aside/logs/daemon-2026-10-23.log` (mode 600). The content of neither was read. Whether a token is in `credentials.json` is UNKNOWN; the file is only 2 bytes. Not read.
- Permission modes on the directories: `~/.aside/` is 755 and its `u/` and `logs/` are 755 or 700. The files that matter (`accounts.json`, `credentials.json`, `settings.json`, `memory-catalog.json`, `daemon-*.log`) are 600. VERIFIED by `ls -l`.
- Unix socket permissions for the daemon: not applicable (no daemon Unix socket was found). The browser's singleton socket exists; its mode was not checked in this run.
- XPC or Mach service: not checked (no `launchctl` or `lsof` for Mach ports was run). UNKNOWN.
- Native messaging hosts: not checked. The directory `NativeMessagingHosts` was not listed. UNKNOWN.

## 5. Permissions and consent

Not tested live. No agent or client action was run, so no dialog was observed.

From DOC (note 05, section 1.5; not re-verified):

- Per-task permission modes: Read only, Guard (default), Full access. Each task is also Default or Incognito.
- Agent rule groups: Sandbox, File permissions, Tool permissions. Tool rules are Allow, Ask, or Deny, and Deny wins.
- Whether those rules apply to `aside mcp` or CLI calls is not documented. This is the key open question. Needs the CLI and a Deny rule test.
- Visible indicator: Agent Tabs group in the sidebar (DOC, changelog v1.0.914.1). Not observed.
- Hard kill switch: not documented. Not tested.

Static cross-check: the strings `Read only`, `Full access`, and `Agent Tabs` appear in the daemon (VERIFIED as strings). `Guard` appears in both the daemon and the framework as a plain word (VERIFIED as strings; not interpreted as a mode name without context).

## 6. Page representation and token cost

Aside snapshot size: BLOCKED. No snapshot tool could be called.

Baseline sizes for the same pages (raw HTML over GET, following redirects). These are not Aside output. They are the upper bound a naive page dump would cost. Tokens are estimated as bytes / 4 and that is a rough estimate.

| Page | Bytes (HTML) | Est. tokens (bytes/4) | Source |
|---|---|---|---|
| https://example.com/ | 577 | ~144 | curl, VERIFIED this run |
| https://en.wikipedia.org/wiki/Web_browser | 415,391 | ~103,848 | curl, VERIFIED this run |
| https://github.com/tonibergholm/yaac (repo page) | 245,228 | ~61,307 | curl, VERIFIED this run |
| Local form test page | not created | not measured | no reader to run it through |
| Aside accessibility snapshot | BLOCKED | BLOCKED | needs CLI or MCP |
| Aside screenshot (file size) | BLOCKED | BLOCKED | needs CLI or MCP |

Representation from DOC: "accessibility snapshots with stable element refs" (note 05, section 1.7, DOC). Not verified.

## 7. Action tools

Not run. The action shapes (click, fill, evaluate, navigation, waits, downloads, tabs, spaces and profiles) are UNKNOWN for this run. note 05's secondary-source claims for `openTab`, `attachBrowserTab`, and the Playwright-style page API stay UNKNOWN.

Static cross-check (VERIFIED as names only): the sdef lists AppleScript commands `open`, `close`, `go back`, `go forward`, `reload`, `stop`, `view source`, `execute`, `save`, `print`, `select all`, `copy selection`, `cut selection`, `paste selection`, `undo`, `redo`, `delete`, `duplicate`, `move`, `make`, `count`, `exists`, `quit`. That is a second automation surface, gated by macOS Automation consent. It was not invoked. Its permission model was not tested.

## 8. Notable for our design

Verified or inferred from this run:

- The app ships a separate local daemon with its own version, its own bundle id, and a loopback TCP listener. The CLI and MCP surfaces both depend on it. That is one enforcement point, which is the design we also want.
- The daemon checks `Host` and `Origin`. That blocks browser-originated requests from foreign origins, which matches the note 05 concern about DNS rebinding and web-page access to the port.
- The daemon answers an unauthenticated `GET /` on loopback. Any local process can reach the port. This supports note 05 R1: keep the control surface off TCP and use a Unix socket with `0600` and a peer-uid check.
- The daemon has `disable-library-validation` and JIT entitlements. For us, the lesson is to keep the control daemon's entitlements minimal.
- The CLI is not on PATH by default. For us, the install step has to be explicit and tested.
- Aside uses a Node.js daemon with a native addon and an embedded MCP client. For us, a Rust core is the enforcement point, and the MCP surface should be a thin stdio forwarder, not a second browser-capable process.
- Password-manager and memory state live under `~/.aside/` with mode 600 on the sensitive files. For us, state files for agents should use the same restrictive modes.

From DOC (not re-verified here):

- Approvals depend on task text, not on an enforced policy (note 05, section 1.5).
- No audit log documented. No kill switch documented.

## 9. Settles and still unknown (against note 05)

Settled in this run:

- A5 (daemon transport): PARTLY SETTLED. The daemon listens on loopback TCP `127.0.0.1:21420` and the browser's network helper is its only observed client. The CLI and `aside mcp` side is still unknown because the CLI is absent. Not a Unix socket.
- Host and Origin checks on the daemon port: SETTLED (headers only; browser CORS not tested).
- Unauthenticated read on the daemon port: SETTLED (GET / returns 200, no credential sent).
- Chromium version of the app: INFERRED 154 from a UA constant.
- The daemon is Node.js based: INFERRED (strings and a `.node` addon).
- CLI install targets: names found (`~/.local/bin`, `~/.aside/bin`); CLI not installed.
- Entitlements and signing: SETTLED (VERIFIED).

Still unknown:

- A6: tools/list over stdio. BLOCKED (needs CLI). Method names exist in the daemon (VERIFIED as strings), but the tool list is UNKNOWN.
- Tool count, names, and schemas. UNKNOWN.
- Whether the MCP transport is stdio only, or also HTTP on the daemon port. UNKNOWN; the daemon port returns JSON to GET, and the MCP path is not known.
- Permission model for MCP calls. UNKNOWN (needs CLI).
- Token: whether a token is required for an authenticated route, and how it is stored. UNKNOWN. Only file names and modes were checked.
- A8: snapshot size. BLOCKED. Baselines are in section 6.
- Console and network tools: UNKNOWN.
- Aside's audit log: UNKNOWN.
- Kill switch behavior: UNKNOWN.
- Whether `aside login` needs a login before MCP works: UNKNOWN.

## 10. Verdict and recommendations for yaac

Verdict: Aside's agent surface is a Node daemon on loopback TCP, with a CLI and MCP in front of it. The daemon's Host and Origin checks are good. Its unauthenticated loopback reachability and its wide entitlements are what we should not copy. The rest of the agent behavior (tools, permissions, snapshot cost) is not established by this run.

Copy conceptually:

- One local daemon as the only control point, with the CLI and the MCP server as thin clients. Matches note 05, section 5.1 (Option A).
- Host and Origin checks on any local HTTP endpoint, as a second layer. Use them only if an HTTP endpoint exists.
- Restrictive file modes (600) on state files and logs.

Avoid:

- Loopback TCP as the control transport. Use a Unix socket at `0600` in a `0700` directory, with a peer-uid check (note 05 R1).
- `disable-library-validation`, JIT, and unsigned-executable-memory entitlements on the control daemon.
- A control process that is also an MCP client to external servers. Keep the agent surface and the outbound MCP client in separate processes.
- Permission rules that depend on task text. Enforce grants and confirmations in the core (note 05 R5).
- Installing the CLI by a remote script that writes into a user PATH directory without a visible step.

Before freezing the yaac tool schema, run the still-open items on a machine where the CLI is installed by the user: A6 (`tools/list`), the Deny-rule test for MCP calls, the snapshot size test (A8), and a check of whether the daemon's GET / body and any authenticated route need a token.

## Verdict on this run

Partial. The install facts, daemon architecture, and Host/Origin behavior were settled. The CLI, MCP tool list, permissions, action tools, and snapshot size were not, because the CLI is not installed and the install step was out of scope for this run. The user's action needed to unblock them: install the CLI from Aside's documented installer and approve the device in Aside Settings, if the owner wants this run completed.
