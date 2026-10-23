# 05. Agent interface for external agents

Question: what should yaac's agent-facing control surface (a CLI and an MCP server) look like, and how should it map onto the Rust core and CEF?

Scope: read-only research from primary sources. Nothing was built or run. Every URL below was accessed on 2026-10-23 unless a different date is given.

Tags:

- VERIFIED: read in a source file, a header, a protocol file, or a fetched page. Pages fetched with WebFetch were read through a summarizer, so quotes from them are checked against the summary, not byte by byte. These are marked "(WebFetch summary)".
- INFERRED: a reasoned conclusion from VERIFIED facts. The reasoning is given.
- UNKNOWN: not determined. Each UNKNOWN names the experiment that would settle it.

Repository snapshots used for source reading (shallow clones, read only, in /tmp):

- ChromeDevTools/chrome-devtools-mcp at ada7963 (2026-10-23)
- microsoft/playwright-mcp (default branch, 2026-10-23)
- vercel-labs/agent-browser (default branch, 2026-10-23)
- browser-use/browser-use (default branch, 2026-10-23)
- chromiumembedded/cef at 7f8a5ec (2026-10-20)
- Chromium main, read through gitiles (2026-10-23)

## Summary

- Aside's public docs describe a CLI, an MCP mode (`aside mcp`), and a REPL. They do not list MCP tools, their parameters, the transport, or an audit log. Most of the Aside detail that is documented is about permissions, password handling, and a sidebar group for agent tabs.
- The prior art converges on accessibility-tree snapshots with element refs, a small tool set, and a stdio MCP server or a CLI with a per-user daemon. The security features are uneven. agent-browser has the most complete documented controls (domain allowlist that covers sub-resources, action policy, confirmation categories).
- CEF exposes `ExecuteDevToolsMethod` and `AddDevToolsMessageObserver` on `CefBrowserHost`. These work without a DevTools front end. CEF passes only `--remote-debugging-port` to Chromium. `--remote-debugging-pipe` has no CEF setting.
- Chrome 136 blocks remote debugging on the default profile. Chromium's source gates that check on Google Chrome branding. CEF builds use Chromium branding, so the check is probably off for us. This is INFERRED and needs a test.
- Recommendation: a Rust core command API served over a per-user Unix domain socket, with Space-scoped grants. The web UI, the `yaac` CLI, and `yaac mcp` are clients of that API. CDP stays internal. No TCP listener.

## 1. Aside

Primary sources: https://docs.aside.com/help/developers.md, https://docs.aside.com/help/security.md, https://docs.aside.com/help/tasks.md, https://docs.aside.com/help/privacy.md, https://docs.aside.com/help/passwords.md, https://docs.aside.com/help/password-manager.md, https://docs.aside.com/help/troubleshooting.md, https://docs.aside.com/changelog/native.md, https://aside.com/blog/developers (developer blog dated 2026-06-22), and https://docs.aside.com/llms.txt (the page index). The `.md` variants of the docs pages were fetched directly.

Secondary source: https://github.com/steggdev/aside-browser-mcp (MIT, pushed 2026-08-12). It is a third-party Hermes skill and uses the Aside CLI. Its claims are tagged "UNKNOWN (secondary source)" below.

### 1.1 Documented commands and surfaces

- Install: `curl -fsSL https://releases.aside.com/install.sh | bash` on macOS, a signed PowerShell installer on Windows. Update with `aside --update`. (VERIFIED, developers.md)
- Agent task: `aside "<prompt>"` starts a browser agent session from the terminal. (VERIFIED, developers.md)
- Continue a session: `aside --session <session-id> "<prompt>"`. (VERIFIED, developers.md)
- Accounts: `aside account list`, `aside account status [id]`, `aside account use <id>`, and `--account <id>` on `aside` and `aside exec`. `aside exec` takes a model flag (`-m`). (VERIFIED, developers.md)
- Login: `aside login --email <addr>` with `ASIDE_CLI_AUTH_PATH` set to an absolute file path. The login is approved in Aside Settings > Account > Devices. (VERIFIED, developers.md)
- MCP: `aside mcp`. The documented client config is `{"mcpServers": {"aside": {"command": "aside", "args": ["mcp"]}}}`. The config format implies stdio transport (INFERRED from the command and args form).
- REPL: `aside repl "<js>"`, for example `const p = await openTab('https://example.com')`. The docs say it gives "page", tabs, locators, screenshots, downloads, and JavaScript, and that it exposes "the same page snapshots and stable element refs that the browser agent uses". (VERIFIED, developers.md and blog/developers)
- Permission modes per task: `Read only`, `Guard` (default), `Full access`. Each task also has `Default` or `Incognito` mode. (VERIFIED, security.md and tasks.md)
- Follow-ups: `Queue` applies a message after the current run. `Steer` injects it into the active run. (VERIFIED, tasks.md)

### 1.2 MCP tools, parameters, and REPL API

- MCP tool names and parameter schemas are not documented. (UNKNOWN; experiment: start `aside mcp` and send `tools/list` over stdio, then record the schema.)
- UNKNOWN (secondary source, steggdev README): the Hermes side exposes a tool named `mcp_aside_repl`, which suggests the Aside MCP server exposes a single `repl` tool (INFERRED from the Hermes naming prefix). The README also lists these REPL functions: `listBrowserTabs()`, `attachBrowserTab(targetId)`, `openTab(url)`, and a Playwright-style page API with `goto`, `click`, `fill`, `evaluate`, `content`, `title`, `url`, `screenshot`, `snapshot`, `frames`, `pdf`, and `locator`. (UNKNOWN, secondary source: https://github.com/steggdev/aside-browser-mcp README)
- UNKNOWN (secondary source, same README): tabs created with `openTab()` belong to the `aside repl` process and close when the command exits. Attaching to an existing tab and navigating it persists. The README recommends keeping one spare tab open. (not checked against the app)

### 1.3 Session model

- `--session <id>` continues a session. The id format and expiry are not documented. (UNKNOWN; experiment: create two sessions and inspect the ids.)
- Task transcripts are saved in the task folder. Generated files stay until the task or folder is deleted. Incognito tasks do not keep normal profile state. (VERIFIED, tasks.md)
- Whether a REPL session is a separate kind of session from an agent session is not documented. (UNKNOWN)

### 1.4 Daemon, transport, and connection

- The app runs a "local service". Its log is `.aside/logs/daemon-YYYY-MM-DD.log`. The troubleshooting page names an installation key that protects local data, and says that on Windows the CLI signs in to the browser with a key stored under the Windows user who runs the browser. (VERIFIED, troubleshooting.md, "Aside keeps restarting" and "CLI cannot open the Windows installation key")
- CLI credentials live in `~/.aside/cli/auth.json` unless `ASIDE_CLI_AUTH_PATH` overrides the path. (VERIFIED, developers.md)
- The transport between the CLI or MCP process and the app is not documented. Port or socket, the socket path, and its permissions are all UNKNOWN. (UNKNOWN; experiment: with the app running on macOS, run `lsof -nP -iTCP -sTCP:LISTEN` and `lsof -U` filtered on the Aside processes, then `ls -l` the socket path.)
- The app is not installed on this machine, so the experiment has not been run.

### 1.5 Authentication and permissions

- Settings > Agents has three rule groups: Sandbox, File permissions, and Tool permissions. Tool rules are Allow, Ask, or Deny, and Deny wins. (VERIFIED, security.md)
- Task-level settings sit on top of agent defaults. (VERIFIED, security.md)
- Saved passwords are filled by autofill and are not shown to the agent. Full access "does not expose saved password values". Access policy is Always allow, While unlocked, or Never. Agents cannot use the password manager in incognito sessions. (VERIFIED, security.md and password-manager.md)
- Approval for sensitive actions depends on the task. The password-manager page says sensitive actions "should still wait for your confirmation when the task asks for approval". The side-panel page uses prompts such as "stop before changing any billing settings". Nothing in the docs says the app enforces a confirmation for a class of actions. (VERIFIED, password-manager.md and side-panel.md)
- Whether the Settings > Agents rules apply to calls made through `aside mcp` or the CLI is not documented. (UNKNOWN; experiment: set a tool rule to Deny, then call the same action through MCP and through the CLI.)

### 1.6 Profiles and logged-in sessions

- Default task mode runs in the normal browser profile. Incognito does not. (VERIFIED, tasks.md)
- Multi-account: each CLI command runs under one selected account. Agents once used only the first profile in a multi-profile setup. The fix is in the changelog. (VERIFIED, developers.md and changelog/native.md)
- Agents act inside the signed-in browser by default. The docs' examples sign in through saved credentials, which means the agent works in the user's session. (INFERRED from the Default mode and the autofill examples)

### 1.7 Evidence returned

- Accessibility snapshots with stable element refs: VERIFIED (blog/developers, REPL section).
- Screenshots: VERIFIED (blog/developers, REPL section). Downloads: VERIFIED (same).
- Log links, screenshot folders, and PR comment drafts as task output: VERIFIED (blog/developers, use-case section).
- Console and network: the docs mention "console issues" only in a QA prompt example. Whether a console or network tool exists is UNKNOWN. (UNKNOWN; experiment: list the MCP tools once the schema is read.)
- Output size limits, screenshot format, and where CLI-run screenshots are stored are not documented. (UNKNOWN)

### 1.8 Keeping the user informed

- Tabs used by external agents through MCP or CLI are grouped in one "Agent Tabs" group in the sidebar. (VERIFIED, changelog/native.md, v1.0.914.1)
- Selecting an agent tab lets you use it as a regular page without pressing Take over. Tabs that were taken over automatically return to the Agent Tabs group when you switch away. (VERIFIED, changelog/native.md, v1.0.914.1)
- Approval dialogs appear in the Chat section, and they close when they lose focus. (VERIFIED, changelog/native.md)
- Waiting tasks show a status and can ask for a reply, approval, or app event. (VERIFIED, tasks.md)
- Stop control: Steer "can adjust" a run and can stop an action. The docs do not describe a hard stop or kill switch. (VERIFIED wording, tasks.md; no hard stop documented)
- The docs do not describe a visual marker on the controlled page, an audit log, or a notification when an external client connects. (UNKNOWN)

### 1.9 Aside does not document

- MCP tool list, schemas, and the REPL API beyond the examples above.
- Transport, port or socket, socket permissions, token lifetime, and session id format.
- Whether the permission rules apply to MCP and CLI calls.
- An audit log of agent actions.
- Kill switch behavior.
- Network and console exposure.
- Output formats and size limits.
- Whether analytics sharing (on by default, per privacy.md) includes agent activity.

## 2. Comparable prior art

### 2.1 Chrome DevTools MCP (ChromeDevTools/chrome-devtools-mcp, Apache-2.0)

- License: Apache-2.0 (https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/LICENSE; repository license API, 2026-10-23).
- Tools: about 60 across categories (input, navigation, emulation, performance, network, debugging, memory, extensions, third-party, WebMCP, PWA, WebAuthn). Core input and page tools are `click(pageId, uid, dblClick?, includeSnapshot?)`, `fill(pageId, uid, value)`, `fill_form`, `type_text`, `press_key`, `navigate_page(pageId, type, url?, ignoreCache?, timeout?)`, `new_page(url, background?, isolatedContext?)`, `list_pages`, `select_page`, `take_snapshot(pageId, filePath?, verbose?)`, `take_screenshot`, `evaluate_script`, `list_console_messages`, `list_network_requests`, and `get_network_request`. (VERIFIED, docs/tool-reference.md in the repository, 2026-10-23)
- Page representation: an accessibility-tree text snapshot with uids. The tool docs say "Prefer taking a snapshot over taking a screenshot." Source: `src/TextSnapshot.ts` calls Puppeteer's `accessibility.snapshot` with a 10 s timeout. (VERIFIED, source)
- Connection: the default launch uses Puppeteer with `pipe: true` (`src/BrowserManager.ts`, around line 281). That is Puppeteer's pipe transport, which uses `--remote-debugging-pipe` on Chrome (INFERRED from the pipe option). Other modes are `--autoConnect` (Chrome 144 or later, needs `chrome://inspect/#remote-debugging`), `--browserUrl`, and `--wsEndpoint`. (VERIFIED, docs/configuration.md and source)
- CLI and daemon: the CLI talks to a background daemon over a Unix socket on macOS and Linux, and a named pipe on Windows. The socket lives under `XDG_RUNTIME_DIR/<app>` or `/tmp/<app>-<uid>`. The daemon listens with `readableAll: false` and `writableAll: false`. The pid file is opened with mode 0600 and `O_NOFOLLOW`. (VERIFIED, src/daemon/utils.ts, src/daemon/daemon.ts, and docs/cli.md)
- Token cost: the design principles say "Token-Optimized" and "Reference over Value". Heavy output (screenshots, traces) goes to a file path. (VERIFIED, docs/design-principles.md) The config offers `--slim` (3 tools) and smaller screenshots to reduce context. (VERIFIED, docs/configuration.md)
- MCP vs CLI: the project ships a CLI with a skill and calls it "particularly useful for debugging or when you want an agent to generate scripts". No measured token comparison is given. (VERIFIED, docs/cli.md; no measurement UNKNOWN)
- Security statements: "exposes content of the browser instance to the MCP clients" and "Avoid sharing sensitive or personal information" (README Disclaimers). The CLI "enables unrestricted filesystem access by default", limited with `--workspace`. Optional `--allowedUrlPattern` (Chrome 149 or later) blocks navigation outside the patterns. `--redactNetworkHeaders` redacts sensitive headers. Usage statistics go to Google, and the Chrome opt-out does not cover them. (VERIFIED, README and docs/configuration.md and docs/cli.md)

### 2.2 Playwright MCP (microsoft/playwright-mcp, Apache-2.0)

- License: Apache-2.0 (repository license API, 2026-10-23).
- Tools: about 70, generated into the README. Core ones are `browser_snapshot` (read-only, optional `target` ref), `browser_click(element, target, doubleClick, button, modifiers)`, `browser_type`, `browser_fill_form`, `browser_navigate`, `browser_navigate_back`, `browser_tabs`, `browser_evaluate`, `browser_take_screenshot` (read-only), `browser_console_messages(level, all, filename)` (read-only), `browser_network_requests` (read-only), `browser_cookie_*` (get and list read-only, set, delete, and clear are not), storage tools, `browser_route` (network interception), `browser_run_code_unsafe`, `browser_pdf_save`, tracing and video tools, and coordinate mouse tools behind `--caps vision`. Every tool carries a read-only flag. (VERIFIED, README "Tools" section)
- Each action takes an `element` string, described as "Human-readable element description used to obtain permission to interact with the element", plus a `target` ref. The `element` field is meant for a client permission hook. (VERIFIED, README)
- Page representation: the accessibility tree. The README says the server "uses Playwright's accessibility tree, not pixel-based input". Vision is opt-in. (VERIFIED, README)
- Connection: Playwright launches its own browser, with `--user-data-dir` (persistent) or `--isolated` (in memory). `--cdp-endpoint` attaches to an existing browser. `--extension` connects to a running Chrome or Edge through the Playwright Extension, which the README says reuses logged-in sessions. (VERIFIED, README configuration section)
- Token cost and MCP vs CLI: the README says CLI calls are "more token-efficient: they avoid loading large tool schemas and verbose accessibility trees". The MCP is kept for "persistent state, rich introspection". This is the project's claim, not a measurement. (VERIFIED wording)
- Security: "Playwright MCP is **not** a security boundary." `--allowed-hosts` checks the Host header of its HTTP mode. `--allowed-origins` "does not serve as a security boundary". `--blocked-origins` is checked before the allowlist. The `secrets` option "is a convenience and not a security feature". (VERIFIED, README security section and option list)
- Playwright CLI (microsoft/playwright-cli, Apache-2.0, README read 2026-10-23): named sessions with `-s=<name>` or `PLAYWRIGHT_CLI_SESSION`, `--persistent` to keep the profile on disk, an automatic shutdown after an hour without commands in headless mode, and a live "session grid" with screencast previews. Its transport is UNKNOWN (not read in source).

### 2.3 agent-browser (vercel-labs/agent-browser, Apache-2.0)

- License: Apache-2.0 (repository license API, 2026-10-23).
- Command surface: `agent-browser snapshot` returns the accessibility tree with refs such as `@e2`. Refs drive `click`, `fill`, `get text`, and more. A `batch` command runs several commands in one invocation. (VERIFIED, README)
- Covering-element errors: a click that another element covers fails early and reports the covering element. The docs say to dismiss it and take a fresh snapshot. (VERIFIED, README around line 101)
- MCP: `agent-browser mcp` over stdio, newline-delimited JSON-RPC. Tool profiles are `core` (default), `network`, `state`, `debug`, `tabs`, `react`, `mobile`, and `all`. Tool names are prefixed `agent_browser_`, for example `agent_browser_snapshot`, `agent_browser_click`, and `agent_browser_eval`. Each tool takes typed fields, including `allowedDomains`. (VERIFIED, README "MCP Server")
- Connection: it launches Chrome for Testing, or attaches with `--cdp <port|url>`. The daemon socket directory is `AGENT_BROWSER_SOCKET_DIR`, otherwise `XDG_RUNTIME_DIR/agent-browser`, otherwise `~/.agent-browser`. Socket file permissions are UNKNOWN (the listener code was not read). (VERIFIED, cli/src/connection.rs lines 101 to 126, and README)
- Security controls, all opt-in: content boundary markers (`--content-boundaries`), a domain allowlist (`--allowed-domains`) that blocks sub-resource, WebSocket, EventSource, and `sendBeacon` requests to other domains and disables WebRTC, an action policy file (`--action-policy`), confirmation categories (`--confirm-actions eval,download`), an encrypted auth vault whose secrets stay out of process arguments, and `--max-output`. (VERIFIED, README "Security features" and "Authentication")
- The README's own warning: "`--remote-debugging-port` exposes full browser control on localhost. Any local process can connect." (VERIFIED, README security notes)
- Token cost: `screenshot --if-changed` skips unchanged images, `--max-output` caps output, and WebMCP summaries are capped at 16 tools and 4 KiB. (VERIFIED, README)

### 2.4 browser-use (browser-use/browser-use, MIT)

- License: MIT (repository license API, 2026-10-23; LICENSE file).
- Page representation: DOM serialization with indexed clickable elements. `browser_use/dom/service.py` calls `Accessibility.getFullAXTree` through the CDP client, then serializes the tree for the LLM. (VERIFIED, source, lines 379 and 1110 to 1138)
- Connection: a `cdp_url` parameter on the browser session, which takes a WebSocket URL. Local launch is the default, and cloud browsers are optional. (VERIFIED, browser_use/browser/session.py lines 213 and 407 to 410)
- Scope: `allowed_domains` and `prohibited_domains` on the browser profile, with prohibited domains checked after allowed ones. (VERIFIED, browser_use/browser/profile.py lines 628 to 635)
- Secrets: `sensitive_data` replaces typed secrets with placeholders. The README warns that "page snapshots and results are saved as they were and can still contain them". (VERIFIED, README around line 373, and browser_use/agent/service.py line 152)
- The CLI and "Browser Harness" are in a separate repository, which was not read. (UNKNOWN)

### 2.5 WebMCP (web pages expose tools to agents)

- Status: a W3C Community Group draft. The spec header says `Status: CG-DRAFT` and `Level: None`. The group report is `cg-report`. (VERIFIED, webmachinelearning/webmcp index.bs and w3c.json, 2026-10-23)
- API: `document.modelContext.registerTool()` and `getTools()`. Cross-origin frames need an `exposedTo` option, and the `tools` Permissions Policy controls frame access. (VERIFIED, webmachinelearning/webmcp README)
- Browser support: "An Origin Trial is live in Chrome 149" and "Edge 150", and a local development flag `about:flags#enable-webmcp-testing`. Firefox has a standards-position issue. (VERIFIED, webmachinelearning/webmcp implementation-status.md)
- Chrome's blog post on the origin trial is dated 2026-06-09. It names no API and says nothing on user confirmation. (VERIFIED, https://developer.chrome.com/blog/ai-webmcp-origin-trial, WebFetch summary)
- Security: the spec says "Preventing an agent from being manipulated by an untrusted site ... is primarily the responsibility of the agent's own security model". Hints such as `readOnlyHint`, `consequentialHint`, and `untrustedContentHint` can be omitted or misrepresented by a site. (VERIFIED, index.bs, Security section)
- Chrome DevTools MCP ships `list_webmcp_tools` and `execute_webmcp_tool` behind a flag. agent-browser ships `webmcp list` and `webmcp invoke` and marks page metadata as untrusted. (VERIFIED, both READMEs)

### 2.6 Cross-cutting: representation, connection, and cost

- Representation: all five tools use an accessibility tree with refs or uids as the main action target. Screenshots are secondary. browser-use also serializes a DOM view with indexes.
- Browser connection: Puppeteer pipe (Chrome DevTools MCP), Playwright-managed launch or extension (Playwright MCP), CDP port or WebSocket (agent-browser, browser-use), Playwright-managed sessions (Playwright CLI).
- Agent-facing transport: stdio for the MCP servers (Chrome DevTools MCP, Playwright MCP by default, agent-browser `mcp`). Unix socket daemons for the CLIs (Chrome DevTools MCP, agent-browser). Playwright MCP also has an HTTP mode on a port, and its Docker example binds `--host 0.0.0.0` (VERIFIED, README). That is the TCP pattern this note recommends against.
- Token cost: every project makes the claim in its own words. No project in this set publishes a measurement. (UNKNOWN; experiment: record snapshot size in bytes and tokens for 5 pages, once with and once without `verbose`.)

## 3. CEF mechanisms

Sources: CEF headers from https://github.com/chromiumembedded/cef at 7f8a5ec (2026-10-20), `include/cef_browser.h`, `include/cef_devtools_message_observer.h`, `include/cef_accessibility_handler.h`, `include/cef_render_handler.h`, `include/cef_request_context.h`, `include/internal/cef_types.h`. The CEF version pinned by cef-rs is 154.5.0. CEF master tracks Chromium 156.0.8078.0 (`CHROMIUM_BUILD_COMPATIBILITY.txt`, VERIFIED). The spikes used cef 154.5.0 and cefclient 156.0.7 (docs/decisions.md).

### 3.1 DevTools methods and observers

- `CefBrowserHost::ExecuteDevToolsMethod(message_id, method, params)` sends a method call. `SendDevToolsMessage` sends raw JSON. `AddDevToolsMessageObserver` receives results and events. (VERIFIED, cef_browser.h)
- The header says "Usage of the SendDevToolsMessage, ExecuteDevToolsMethod and AddDevToolsMessageObserver methods does not require an active DevTools front-end or remote-debugging session." (VERIFIED, cef_browser.h)
- `CefDevToolsMessageObserver` has `OnDevToolsMessage`, `OnDevToolsMethodResult`, `OnDevToolsEvent`, and optional `OnDevToolsAgentAttached` and `OnDevToolsAgentDetached`. (VERIFIED, cef_devtools_message_observer.h)
- Scope: these calls are methods on one `CefBrowser`. The DevTools agent is per browser. Whether `Target.*` or browser-level commands reach other targets through it is UNKNOWN. (UNKNOWN; experiment: call `Target.getTargets` from one browser and check whether it lists other tabs.)
- cef-rs 154.5.0 binds all of these. The generated macOS arm64 binding declares `execute_dev_tools_method`, `send_dev_tools_message`, `add_dev_tools_message_observer`, and the `DevToolsMessageObserver` wrapper macro. (VERIFIED, `cef-154.5.0+154.0.34/src/bindings/aarch64_apple_darwin.rs` in the cargo registry, lines 12611 to 12624 and 3697 onward)

### 3.2 Remote debugging switches

- `CefSettings.remote_debugging_port`: values from 1024 to 65535. The command-line switch is `remote-debugging-port`. A value of 0 selects an ephemeral port, and CEF writes it to `DevToolsActivePort` in the cache directory. (VERIFIED, include/internal/cef_types.h)
- CEF forwards only the port. `libcef/common/chrome/chrome_main_delegate_cef.cc` lines 332 to 336 append `remote-debugging-port` when the setting is in range. `libcef/` has no reference to `remote-debugging-pipe` (grep of the CEF repository, VERIFIED).
- INFERRED, probably works: `--remote-debugging-pipe` should be honored under CEF's Chrome runtime. Chromium's content layer has a pipe handler (`content/browser/devtools/devtools_pipe_handler.cc`, VERIFIED). The Chrome layer starts it in `RemoteDebuggingServer::Start` (`chrome/browser/devtools/remote_debugging_server.cc` around line 353, VERIFIED). The reason: the port and the pipe servers start in the same function, and CEF already relies on that function for `remote-debugging-port` (INFERRED from the CEF switch wiring). CEF's Alloy runtime path is UNKNOWN. (Experiment A3.)
- cef-rs exposes `remote_debugging_port` in the generated `Settings` binding. (VERIFIED, aarch64_apple_darwin.rs line 520)

### 3.3 Chrome 136 restriction and CEF

- The change: from Chrome 136, `--remote-debugging-port` and `--remote-debugging-pipe` "will no longer be respected if attempting to debug the default Chrome data directory". The switches must come with `--user-data-dir` pointing at a non-default path. Chrome for Testing keeps the old behavior. Published 2025-03-17. (VERIFIED, https://developer.chrome.com/blog/remote-debugging-port, WebFetch summary)
- Stated reason: attackers have used remote debugging to extract cookies. The blog says Google watches "the techniques and behaviors used by infostealers", but it does not name infostealers as the reason for this change. (VERIFIED, WebFetch summary)
- Source: `chrome/browser/devtools/remote_debugging_server.cc` in Chromium main, lines 162 to 182. The default-profile check is compiled only when `BUILDFLAG(GOOGLE_CHROME_BRANDING)` is true. Otherwise it stays off unless a test-only flag is set. The check also runs on the pipe switch. (VERIFIED, source read from gitiles, 2026-10-23)
- Chromium's default is `is_chrome_branded = false` (`build/config/chrome_build.gni` line 12, VERIFIED, Chromium main). CEF's repository sets no branding override in `tools/` or `libcef/`. CEF patches reference `GOOGLE_CHROME_BRANDING` in at least the crashpad and policy patches (VERIFIED, `patch/patches` in the CEF repository, first matches only). The `libcef/`, `tools/`, and `CMakeLists.txt` searches found no branding override (VERIFIED).
- Conclusion: CEF builds that use Chromium branding should not apply the default-profile check. This is INFERRED for the 156 source and for the 154 pin. (INFERRED)
- Experiment: run a CEF build with `--remote-debugging-port=9222` and no `--user-data-dir`, then `curl localhost:9222/json/version`. A response means the check is off. (Not run.)
- Even if the check is off, the design should not depend on it. The rule should be a dedicated profile directory for agent-driven state.

### 3.4 Accessibility tree: two routes

- Route A, CDP `Accessibility` domain (protocol JSON from https://github.com/ChromeDevTools/devtools-protocol, `json/browser_protocol.json`, read 2026-10-23): `Accessibility.enable` "turns on accessibility for the page, which can impact performance until accessibility is disabled". `getFullAXTree` takes optional `depth` and `frameId`. `getRootAXNode`, `getAXNodeAndAncestors`, and `getChildAXNodes` state that they require `enable()` first. `getFullAXTree` does not state that requirement. (VERIFIED)
- Route A through yaac: `ExecuteDevToolsMethod("Accessibility.enable")`, then `getFullAXTree`, then `disable` after the snapshot. Whether `getFullAXTree` works without `enable` is UNKNOWN. (UNKNOWN; experiment: call it cold.)
- Route B, `CefAccessibilityHandler`: `CefRenderHandler::GetAccessibilityHandler()` returns it. The handler gets `OnAccessibilityTreeChange(CefValue)` and `OnAccessibilityLocationChange(CefValue)`. The header says that for windowless browsers, accessibility runs in "TreeOnly mode", events are passed to the handler, and "platform accessibility objects are not created". (VERIFIED, cef_accessibility_handler.h, cef_render_handler.h, cef_browser.h around line 979)
- The handler hangs off the render handler, which only off-screen (windowless) browsers use. Our Views-based windows are not windowless. So Route B is probably unavailable to us. (INFERRED)
- The format of the `CefValue` tree is not documented in the header. (UNKNOWN)
- Decision: use Route A. It does not depend on off-screen rendering.
- `Page.captureSnapshot` "Returns a snapshot of the page as a string", with MHTML as the full serialization format. It is not an accessibility tree. (VERIFIED, `browser_protocol.json`, Page domain, read 2026-10-23)

### 3.5 Request contexts

- `CefRequestContext` has its own cookie manager (`GetCookieManager`) and cache path. An empty cache path gives an in-memory "incognito" context. `CreateContext` and `CreateContextShared` create contexts, and the shared form shares storage with another context. (VERIFIED, cef_request_context.h)
- That the cookie store is fully separate between two contexts is INFERRED from the per-context cookie manager and cache path. (INFERRED; experiment: set a cookie in context A and read it from context B.)

## 4. Security model

### 4.1 Threats and the sources behind them

- Lethal trifecta: an agent is dangerous when it has access to private data, sees untrusted content, and can communicate externally. Simon Willison, 2025-06-16, https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/ (VERIFIED, WebFetch summary)
- Prompt injection in a logged-in browser: Brave tested Perplexity's Comet. Hidden text on a page made the agent use the user's session, read a one-time code from Gmail, and send both to the attacker. Brave's stated fixes: separate user instructions from page content, check planned actions for alignment, require explicit confirmation for sensitive tasks, and isolate agentic browsing from regular browsing. Published 2025-08-20, https://brave.com/blog/comet-prompt-injection/ (VERIFIED, WebFetch summary). Brave also notes that same-origin policy and CORS do not stop this, because the agent acts with the user's privileges.
- Anthropic, Claude in Chrome: "Browser use without our safety mitigations showed a 23.6% attack success rate", reduced to 11.2% with mitigations in autonomous mode. On a browser-specific challenge set the rate went "from 35.7% to 0%". Claude asks before "publishing, purchasing, or sharing personal data". Site-level permissions can be set in Settings. Dated 2025-08-25 with updates, https://claude.com/blog/claude-for-chrome (VERIFIED, WebFetch summary; the original anthropic.com URL redirects here)
- Microsoft Edge, Actions in Edge (support page, no date shown, read 2026-10-23, WebFetch summary): the feature is "experimental" and may be "affected by malicious hidden instructions on web pages". It "may also ask you to approve a site if it doesn't seem directly related to your task", and "Copilot will ask for your attention and supervision for certain actions". https://support.microsoft.com/en-gb/topic/copilot-actions-in-edge-5ed5e17e-42df-40a3-984a-20420eba86e2 (VERIFIED, WebFetch summary). One search for a Microsoft security blog post on this (2026-10-23, "Microsoft Edge Copilot Mode agent actions prompt injection security blog microsoft.com") found none. (UNKNOWN for a Microsoft security post)
- Google Chrome, agentic browsing (2025-12-08): a User Alignment Critic reviews each planned action, read-only origins and read-writable origins are separated, the agent asks for confirmation before certain sensitive sites, and the page says "logged-in sites vulnerable to data exfiltration" when the agent runs on a local browser. https://blog.google/security/architecting-security-for-agentic/ (VERIFIED, WebFetch summary)
- OpenAI, ChatGPT Atlas: the primary post returned 403 to WebFetch. A secondhand report quotes the CISO's description of prompt injection as "a frontier, unsolved security problem" and OpenAI's recommendation to use logged-out mode unless the task needs the user's accounts. (UNKNOWN for primary text; UNKNOWN (secondary source) through search results only, https://simonwillison.net/2025/Oct/22/openai-ciso-on-atlas/ and others)
- WebMCP spec: prompt injection is the agent's problem to solve, and the spec cannot enforce it. (VERIFIED, index.bs)

### 4.2 Exposed debugging ports

- Any local process can use a remote-debugging port. Chrome DevTools MCP's docs say "Any application on your machine can connect to this port and control the browser." agent-browser's README says the same. (VERIFIED, both READMEs)
- Host header check: Chromium's DevTools HTTP handler serves only requests whose `Host` header is empty, an IP address, or a localhost name. Other requests get HTTP 500. This is the check that blocks DNS rebinding against the port. (VERIFIED, content/browser/devtools/devtools_http_handler.cc, `RequestIsSafeToServe` and `OnHttpRequest`, Chromium main)
- Origin check on WebSocket: when an `Origin` header is present and is not same-origin and not in `--remote-allow-origins`, the handshake is rejected. Requests with no `Origin` header are not rejected. Browsers send `Origin` on WebSocket handshakes (INFERRED from browser behavior; RFC 6455 section 4.1 is the background, not fetched in this session). A local program usually does not, so the check does not stop local programs. (VERIFIED, devtools_http_handler.cc around lines 838 to 860)
- Playwright MCP uses the same idea in its HTTP mode: `--allowed-hosts` (VERIFIED, README).
- Chrome 136 changed the default-profile behavior (section 3.3).
- Our conclusion: the Host check and the Origin check protect a port from web pages. They do not protect it from local processes. A Unix socket with filesystem permissions protects against other local users, and it is not reachable from a web page at all.

### 4.3 How the tools scope access

| Tool | Scope control | Confirmation | Audit | Notes |
|---|---|---|---|---|
| Chrome DevTools MCP | `--allowedUrlPattern` (Chrome 149 or later), `--workspace` for files | none documented | none documented | Disclaimer that content is exposed to the client. Usage statistics go to Google. |
| Playwright MCP | `--allowed-origins` and `--blocked-origins` | client permission hook through the `element` field | none documented | "not a security boundary" (README) |
| agent-browser | `--allowed-domains` (includes sub-resources, WebSocket, sendBeacon) and action policy file | `--confirm-actions` categories | none documented | Most complete documented set. |
| browser-use | `allowed_domains`, `prohibited_domains` | none documented | none documented | `sensitive_data` placeholders |
| Aside | permission modes Read only, Guard, Full access; Allow, Ask, Deny per tool | task-dependent approval | not documented | Password values hidden from agent. Incognito blocks password manager. |
| WebMCP | `exposedTo`, Permissions Policy `tools` | hints only (`consequentialHint`) | not specified | Spec says the agent is responsible. |
| Edge Actions in Edge (experimental) | trusted-site limits, high-risk site blocking, per-site approval | supervision prompts | not documented | Microsoft support page (WebFetch summary) |
| Google Chrome agent | origin sets, User Alignment Critic | confirmation for sensitive sites | not documented | Describes the layers, not an API. |

### 4.4 Design requirements for yaac

R1. Transport. Unix domain socket on macOS and Linux. The socket directory is created with mode 0700 under a per-user runtime path, and the socket is 0600. The server checks the peer uid on each connection (`getpeereid` on macOS, `SO_PEERCRED` on Linux). No TCP listener, not even on localhost. Windows uses a named pipe with a DACL for the current user, and the name includes the user. Source: chrome-devtools-mcp daemon (above); CDP port risk (section 4.2).

R2. Authentication and enrollment. Each external client enrolls once. The user approves the enrollment in the app, and the client gets a grant token stored in a 0600 file. The token is revocable from the UI and has a short lifetime that renews only while the user keeps the grant. Aside's CLI login already uses this pattern: a code is approved in the app (VERIFIED, developers.md). Same-uid malware can read the token file. That is outside the threat model we can enforce, and the design should say so.

R3. Grants per Space. A grant is a record of (client, Space, scopes, expiry). A Space maps to one request context, so its cookies and storage are separate (section 3.5, INFERRED). Agents get a dedicated agent Space by default. An existing Space with logged-in state is reachable only by an explicit grant.

R4. Scopes. Three levels:
- read: list, snapshot, screenshot, text, console, and network summaries (no bodies).
- act: navigate, click, fill, press, and select, limited to origins in the grant. This matches Google's read-only and read-writable origins and agent-browser's domain allowlist.
- privileged: evaluate, downloads, uploads, and any cookie or storage access. Off by default. Maps to Aside's Full access mode and agent-browser's confirm categories.

R5. Confirmation outside the page. The confirmation dialog is rendered by the trusted web UI, not by page content. Required for: any privileged action, the first act on a new origin unless the grant allows it, form submission that looks like payment, account, or credential change, and downloads. Enforced in the Rust core. Aside's documented approvals depend on the task text, so they are not an enforced policy (section 1.5).

R6. Visible indicator. Tabs driven by an agent appear in a group for that Space, and the Space shows "agent connected" while a grant is live. Aside has one Agent Tabs group for all external agents (VERIFIED, changelog). The per-Space version is our design choice.

R7. Audit log. An append-only local log per command: time, client id, Space, tool, scope, origin, result code, and duration. Page content is not logged by default. Typed values are redacted in password-type fields. A UI page lists the log.

R8. Kill switch. One control per Space revokes every grant for that Space and cancels in-flight commands. The CLI has the same action (`yaac agent stop`). The latency from click to stopped commands needs a measurement. (UNKNOWN)

R9. Page output is untrusted data. Text from pages is wrapped in a labeled boundary and marked `untrusted: true`, following agent-browser's `--content-boundaries`. The spec and the vendors say this is a cue for the model, not a defense (VERIFIED, agent-browser README, WebMCP index.bs).

R10. Secrets. No tool returns a password value. v1 has no password or autofill tool. Playwright's `secrets` option is only a text replacement, and the docs say so.

R11. Cookies. Off in v1. If added later, read scope only, with confirmation, and output redacted. Cookie values never go to MCP output.

R12. Limits. Maximum output size per response (agent-browser `--max-output`), a per-client rate limit, and a per-Space concurrency cap.

R13. Profile separation. Agent-driven state lives in a dedicated context. Writes to the user's default profile happen only through a granted Space. Chrome's 136 restriction does not decide this. We should not rely on the check being off.

## 5. Design implications for yaac

### 5.1 Where the surface lives

Option A: a Rust core command API. The core exposes typed commands over the Unix socket. The web UI and the external clients send the same typed command enum to one dispatcher. Only the transport differs: the web UI uses CEF IPC, and external clients use the Unix socket. `yaac <cmd>` and `yaac mcp` are clients. Advantages: one enforcement point for grants, confirmations, audit, and kill switch. Space and tab state already lives in the core, so tool results match the sidebar. Cost: we write the socket protocol and the client.

Option B: a CDP proxy. A process sits between agents and CEF's DevTools endpoint. Advantages: fastest to ship, and existing MCP servers can be reused. Costs: CDP has no concept of Spaces, the enforcement point is outside the core, and we depend on a remote debugging port, which section 4.2 shows is the risky part.

Option C, recommended: Option A as the public surface, with CDP used inside the core only. The core calls `ExecuteDevToolsMethod` for snapshot, click, and evaluate. Agents never see a debugging port.

### 5.2 One binary or two

Recommendation: one binary with `yaac <cmd>` and `yaac mcp`. Aside uses the same shape: `aside`, `aside mcp`, and `aside repl` ship together (VERIFIED, developers.md). `yaac mcp` is a stdio process that forwards to the running app's socket. If the app is not running, it should say so and not start a second browser. One install path, one version, and one auth path.

### 5.3 Minimum viable tool set for v1

17 tools:

- Spaces and tabs: `spaces_list`, `tabs_list(space)`, `tabs_open(space, url)`, `tabs_close(tab)`, `tabs_focus(tab)`.
- Navigation: `navigate(tab, url | back | forward | reload)`, `wait(tab, condition, timeout)` where condition is load, selector, text, or URL.
- Read: `snapshot(tab)` returns the accessibility tree with refs, bounded by `max_bytes`. `text(tab, ref?)` returns readable text or Markdown. `screenshot(tab, ref?)` writes a file in the Space's artifact folder and returns the path, not the bytes. `console(tab, since)`.
- Act: `click(tab, ref)`, `fill(tab, ref, value)`, `press(tab, key)`, `select(tab, ref, value)`.
- Privileged, off by default, confirmed: `evaluate(tab, expr)`, `download(tab, ref)`.

Refs are valid only for the snapshot that produced them. After navigation or a DOM change, a call with a stale ref returns an error that says to take a fresh snapshot. This follows the Chrome DevTools design principle "Self-Healing Errors" and agent-browser's covering-element error (VERIFIED, both).

Network summaries (URL, method, status, type, size, no bodies) are cheap once the domain is enabled. Include them in v1 if the `Network` domain cost is acceptable. Measure first (UNKNOWN).

Cut from v1: MCP resources, HAR export, performance traces and Lighthouse, heap snapshots, emulation, PDF, video and recording, network bodies, file uploads, cookies, storage, password and autofill, routines and scheduling, the REPL, WebMCP bridging (the spec is still a draft), extension control, multiple accounts, raw CDP passthrough, and any TCP port.

Owner decision needed: whether `evaluate` ships in v1. Agents use it often, and it is also the widest-scope tool.

### 5.4 What the Space model gives us

- Aside groups all agent tabs into one "Agent Tabs" group (VERIFIED). yaac's agent tabs live in the Space they were granted to, so the sidebar tree shows which Space an agent can touch and which tabs it opened.
- Grants are per Space, so revoking one Space does not touch another. Aside's rules are per agent and per task (VERIFIED, security.md).
- The Space maps to one request context, so isolation comes from an existing concept instead of a separate profile system (section 3.5, INFERRED).
- The kill switch sits next to the Space in the sidebar, not in a separate dashboard.

This is a design argument, not a measured advantage. The Aside sidebar behavior is documented only through its changelog.

### 5.5 Risks specific to our stack

- The CEF remote-debugging questions (section 3.2 and 3.3) decide whether we can use a debugging port at all. The design avoids needing one.
- Accessibility on Views windows: Route A works on any browser, but `Accessibility.enable` has a cost. It needs measurement.
- Linux: the Wayland spike result (docs/decisions.md) does not cover the socket or the tab group UI.

## 6. Licensing and reuse

GPL-3.0 compatibility per the FSF license list (https://www.gnu.org/licenses/license-list.html, accessed 2026-10-23, VERIFIED): Apache License 2.0 is "compatible with version 3 of the GNU GPL" and is not compatible with GPL version 2. The Expat (MIT) license is "compatible with the GNU GPL". The "W3C Software Notice and License" is "GPL compatible".

| Source | License | Checked at | GPL-3.0 compatible | Reuse note |
|---|---|---|---|---|
| ChromeDevTools/chrome-devtools-mcp | Apache-2.0 | LICENSE, 2026-10-23 | Yes | Keep license and NOTICE if one exists (none in the clone root) |
| microsoft/playwright-mcp | Apache-2.0 | repository license API, 2026-10-23 | Yes | Same |
| microsoft/playwright-cli | Apache-2.0 | repository license API, 2026-10-23 | Yes | Same |
| vercel-labs/agent-browser | Apache-2.0 | LICENSE, 2026-10-23 | Yes | Same |
| browser-use/browser-use | MIT | LICENSE, 2026-10-23 | Yes | Keep the copyright notice |
| ChromeDevTools/devtools-protocol (protocol JSON) | BSD-3-Clause | repository license API, 2026-10-23 | Yes (VERIFIED: FSF list, #ModifiedBSD, "compatible with the GNU GPL") | Use the protocol definition as data. |
| webmachinelearning/webmcp | W3C Software and Document License per README; repository API reports NOASSERTION | README and API, 2026-10-23 | Yes for the software notice (FSF) | Spec text, not code. |
| steggdev/aside-browser-mcp (secondary) | MIT | repository license API, 2026-10-23 | Yes | Secondary only. Not read beyond the README. |
| Aside (aside.com, Aside Browser and CLI) | Not public. The CLI is distributed as a binary. | install page and docs, 2026-10-23 | Not applicable | Observed behavior and docs only. No code, assets, or binaries read or copied. |

Recommendation: design from the observed behavior and the protocol definitions. If any code is copied from Apache-2.0 or MIT projects, keep the license header and a notice of the change. Prefer a rewrite. No file in this research was copied into the repository.

## Verdict

Recommended v1 scope: a Rust core command API served over a per-user Unix socket, with Space-scoped grants and a kill switch and audit log in v1. Ship one binary with `yaac <cmd>` and `yaac mcp`. Expose 15 tools plus 2 privileged tools: spaces and tabs, navigate and wait, snapshot with refs, text, screenshot, console, click, fill, press, and select. Keep `evaluate` and `download` behind the privileged scope with confirmation. No cookies, no raw CDP, no TCP port.

Top three risks:

1. Prompt injection in a logged-in Space. The lethal trifecta applies directly: the agent sees private data, reads untrusted page text, and can act or send. Mitigations are architectural (Space grants, origin scope, confirmations enforced in the core), not model-based. Vendors report that model-side defenses still leave a nonzero attack rate (Anthropic: 11.2% with mitigations).
2. A debugging port or a local process gets full browser control. Keeping CDP internal and using a socket with filesystem permissions removes the web-page path. Same-uid local malware remains a limit we cannot enforce.
3. CEF facts that are still INFERRED or UNKNOWN: whether the default-profile check is off in our build, whether `Target.*` reaches other tabs through `ExecuteDevToolsMethod`, whether `getFullAXTree` works without `enable`, and whether pipe mode works under CEF. Wrong answers here change the API design, so run the experiments before freezing the tool schema.

Open questions for the owner:

1. Does `evaluate` ship in v1 with confirmation, or does it wait?
2. Can an agent ever use the default profile, or is it limited to Spaces the user creates for agents?
3. Should every act on a logged-in Space need confirmation, or only sensitive origins and forms?
4. Are cookies out permanently, or only out of v1?
5. Is the MCP tool schema a public, versioned contract from the first release?
6. What is the Windows transport: a named pipe with a DACL, or something else?

Experiments that settle the UNKNOWN items (none run yet):

- A1: `ExecuteDevToolsMethod("Target.getTargets")` from one CEF browser. Settles whether the DevTools scope reaches other tabs.
- A2: CEF with `--remote-debugging-port=9222` and no `--user-data-dir`, then `curl localhost:9222/json/version`. Settles the Chrome 136 question for our build.
- A3: CEF with `--remote-debugging-pipe`. Settles whether pipe mode works under CEF.
- A4: `Accessibility.getFullAXTree` on a cold page, then with `enable`. Settles the enable requirement, and with timing, the cost.
- A5: Aside installed on macOS, `lsof` on the daemon during `aside mcp`. Settles Aside's transport.
- A6: `tools/list` on `aside mcp` over stdio. Settles Aside's tool schema.
- A7: two `CefRequestContext` objects, one cookie set. Settles isolation for Spaces.
- A8: snapshot size in bytes and tokens for five pages, with and without verbose. Settles the token claim.

Not verified:

- OpenAI's primary Atlas post (403 on fetch). Only secondhand reporting was read.
- Microsoft's own Edge security post (not found in one search). The Actions in Edge support page was read through WebFetch.
- Playwright CLI transport (README only).
- agent-browser socket file permissions (directory logic read, listener code not read).
- Anthropic, Google, and Brave quotes come from WebFetch summaries, not from the full pages.
- The CEF branding claim is for the 156 source and the 154 pin, by reading. No run.
- Chrome 136 check on CEF has not been run.
- Aside's MCP tools, transport, and permissions for MCP calls.
