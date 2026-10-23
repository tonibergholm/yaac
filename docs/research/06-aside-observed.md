# 06. Aside, observed from outside (run 2)

Date: 2026-10-23. Branch: `aside-analysis-2`. Status: the CLI command tree, the MCP tool list, the snapshot format and sizes, the daemon transport, and the REPL action surface are settled. Permissions for MCP and CLI calls are partly settled (run 3 tested a Deny rule; see section 11). The approval indicator is settled for the Aside window (run 4, section 12). The kill switch is not found: `session stop` does not stop a `repl` run, and the owner check is NEEDS USER. Run 1 (no CLI) is superseded where noted.

Scope rules followed: clean room. No disassembly or decompilation. No copy of Aside's code, prompts, models, or assets into the repo. Aside's tool descriptions and REPL guide were read at runtime and are not reproduced here; the raw captures keep names, parameter shapes, and runtime output only. No read of any file under `~/.aside/` (the CLI reads its own state; this run did not open any file there). No intentional `aside exec` or `aside <prompt>`; one accidental prompt run, see section 0. No `aside memory`, no login, logout, or update. I did not read or list any user tab or window. The CLI's `account status` printed the profile label. The tests ran in the user's only profile, so the public pages I loaded may have left history and cookie entries there (INFERRED). The accidental agent run (section 0) is a further side effect. Test pages: local `127.0.0.1:8765` (served from `/tmp`, stopped at the end), `https://example.com/`, the Wikipedia article "Web browser", and `https://github.com/tonibergholm/yaac`.

Labels: VERIFIED = observed in this run. VERIFIED (run 1) = observed in run 1 and not re-checked. INFERRED = a reasoned conclusion from VERIFIED facts. DOC = from note 05 (Aside's public docs), not re-checked. UNKNOWN = not determined.

Raw captures (small text files): `docs/research/06-aside-observed/`
- `cli-help.txt`: help output of the root and each group command. Leaf `--help` is covered in section 2.
- `cli-exit-codes.txt`: exit codes and output shape for the safe invocations.
- `mcp-initialize.json`, `mcp-tools-list.json` (prose removed, names and schemas kept), `mcp-resources-prompts-errors.json`.
- `repl-probe-transcript.txt`: step-by-step REPL calls and their outputs (Aside runtime warnings removed).
- `snapshot-sizes.txt`: size table and the cap marker.
- `transport-and-auth.txt`: process and socket facts, daemon status document, header-name check.
- `runtime-probes.txt`, `static-inventory.txt`: from run 1.

## Summary

- The CLI is `/Users/tonibergholm/.local/bin/aside`, version 1.26.1023.1115. It is on PATH. The CLI is signed in as the user's account (`account status`, provider google, one profile). VERIFIED.
- `aside mcp` is a stdio MCP server, `serverInfo` name `aside`, protocol 2025-06-18. It exposes four tools: `repl`, `exec`, `browser_profiles`, and `memory_search`. The `--help` text lists only three, because it omits `memory_search`. No tool has annotations. VERIFIED.
- The CLI talks to the daemon over loopback TCP `127.0.0.1:21420`, the same port the browser's network helper uses. The daemon answers `GET /` and `GET /health` with a 231-byte status document and no credentials. Requests with a foreign `Origin` or `Host` get 403. VERIFIED.
- The snapshot is a YAML-like accessibility tree with `[ref=eN]` tokens. A call returns a unified-style diff against the immediately preceding snapshot call on the same page. After a call with different options it returns the full tree. A hard cap of 16 KiB per snapshot applies (not paginated). example.com (577 bytes of HTML) becomes a 270-byte tree. Wikipedia and a 492 KB list page are both cut at 16 KiB. VERIFIED.
- The REPL performed fill, click, keyboard input, a download, navigation, evaluate, and screenshots on test pages without any approval prompt or permission error. The `repl` tool has no permission parameter. VERIFIED. Whether tool rules or the Guard setting apply to `repl` calls: UNKNOWN (no Deny rule was tested).
- Two Aside CLI defects were observed: an unknown word is treated as a paid agent prompt, and a failed REPL script exits with status 0. VERIFIED. One accidental agent session was created and deleted (section 0). Run 4 added a third: `closeTab` returns "closed" while the tab stays in the sidebar.
- Run 4 (section 12): a `repl` call shows an "Agent Tabs" sidebar group and a REPL panel in the Aside window, and brings Aside to the front. `session stop` does not stop a `repl` run. No UI kill switch was found; the owner check is NEEDS USER.

## 0. Incidents, limits, and what was not done

Incident (my error, this run): `aside bogus-cmd` was run to test the exit code of an unknown command. The root command treats unknown words as a prompt (usage `aside [options] [prompt...]`), so this started an agent run. It created one ephemeral session titled "Investigating bogus-cmd failure". The session was idle. I ran `aside session stop` (it printed `idle`, exit 0) and `aside session delete` (it printed `ok`, exit 0). After that, `aside session list` showed only the session from the MCP test. The session got a generated title, so a model call ran. Run 4 confirmed the charge: Settings > Plan & Usage lists `bogus-cmd` on aside/gpt-6.1-sol at 3 credits (section 12.6). It ran with the default permission (Guard; the help says Guard is the default). UNKNOWN: whether it opened a tab in the user's browser. Checking that would have meant listing tabs or reading session transcripts, which the rules forbid. The user should check Aside's usage history for that time, 2026-10-23 07:26 UTC.

Side effect (my test): the download test saved `probe.txt` (29 bytes) to `~/Downloads`. I compared it with my test string, which matched, and removed it. Aside's download history in Profile 0 probably holds an entry (INFERRED).

Other limits:
- The window-level indicator screenshot could not be taken. System Events sees one Aside window, but the CoreGraphics window list returned no Aside windows, so there was no window ID for `screencapture -l`. A region capture would have included the user's other tabs. Blocked; see section 5.
- `aside exec`, the agent tool, and `memory_search` were not called. `browser_profiles` was not called (it lists profile state). `aside memory *` and `aside skills show` were not run. `aside update` was not run (it installs). `aside settings get` was not run (it prints the effective settings).
- The Deny-rule test was not run, because it needs a change to the user's Agent settings. What would be needed: one tool rule set to Deny for `repl`, a call from `aside mcp`, and the same rule reset afterwards.
- The 120-second REPL timeout was not timed.
- Tab lifecycle at process exit (a secondary-source claim) was not tested. Testing it needs listing browser tabs.
- My own step had a test-design error: clicking the Send button of the local form submitted it and navigated to a 404 page, which made later steps fail. Not an Aside defect.
- The REPL's sandbox `fs` refused writes outside its project and session roots (`Path escapes Project and session roots`). VERIFIED. Snapshots were therefore measured in the REPL and the full trees were read back through the tool output.
- Every REPL session runs in the user's only browser profile (`Profile 0`, from `account status`). INFERRED: the test pages loaded with the user's cookies, because the profile is shared. The test pages were public or local, and no user tab was touched. There was no way to choose a separate profile without listing profiles, so isolation was not tested.

## 1. Install facts [run 1, VERIFIED (run 1) unless marked]

- App bundle `/Applications/Aside.app`, bundle id `at.studio.AsideBrowser`, version 1.0.1022.1, minimum macOS 13.0, arm64. VERIFIED (run 1).
- Daemon bundle `at.studio.AsideBrowser.daemon`, plist version 1.26.1023 (VERIFIED, run 1). The running daemon's status document reports version `1.26.1023.5` (VERIFIED, run 2).
- CLI: `aside --version` prints `1.26.1023.1115`. The install directory is `~/.local/bin`, which is on PATH (`command -v aside`). Run 1 found the CLI absent and had no PATH entry, which is superseded. VERIFIED (run 2).
- Chromium major version 154, from the user-agent constant in the framework binary. INFERRED (run 1, not re-checked).
- Code signing: Developer ID, hardened runtime. Entitlements of the daemon include `allow-jit`, `allow-unsigned-executable-memory`, and `disable-library-validation`. VERIFIED (run 1).
- The daemon binary includes the strings `ASIDE_CLI_*` and the install-target paths. The update path is `aside update` (documented). VERIFIED (run 1, strings only).

## 2. CLI command tree (VERIFIED: help text in `cli-help.txt`, and the runs noted)

Root: `aside [options] [prompt...]`. A positional prompt starts an agent run (not run, except the accident in section 0). Options: `--account <id>`, `--host <host>`, `--profile <id>`, `-m/--model`, `-p/--provider`, `-s/--speed` (`default`, `fast`), `--effort` (`off` through `max`, plus `ultrabrowse`), `--permission` (`ask`, `guard`, `full-access`), `--update`, `-V`.

| Command | Purpose (help text) | Options | Run in this session |
|---|---|---|---|
| `account list`, `account status [id]`, `account use <id>` | list and switch CLI accounts | leaf options not captured (leaf --help printed root help) | `list`, `status` (exit 0; shows the signed-in account and its profile label) |
| `login` | sign the CLI in with a code approved in Settings | `--account`, `--email`, `--password`, `--device-code` (default) | not run |
| `logout` | sign out and revoke CLI credentials | -h only | not run |
| `host list`, `host status`, `host use <host>`, `host profiles` | session hosts (Remote Control or cloud) | `profiles` takes options | `status` printed `local` (exit 0). `list` printed a line ending in `: 403` (exit 1). `profiles` not run (profile state). |
| `memory search/list/show/path` | read-only memory | leaf options not captured (leaf --help printed root help) | not run (personal data); help only |
| `skills list`, `skills show <name>`, `skills install` | site skills for `repl`; install writes into coding-agent config | leaf options not captured (leaf --help printed root help) | `list` ran: 13 tab-separated lines (name, description) (exit 0) |
| `exec [prompt...]` | run the browser agent | the root options | not run |
| `repl [code...]` | Playwright-style JavaScript, 120 s timeout | `--account`, `--host`, `--profile` | one-shot `console.log(1+1)` printed `2` and `[ok | 10ms]`, exit 0. A JS syntax error printed `[error | ...]`, exit 0. |
| `mcp` | MCP server over stdio | `--account`, `--host`, `--profile` | run as the MCP server (section 3). Stdin end gives a shutdown event with reason `stdin-end` and exit 0. |
| `guide [topic]` | usage guide; topic `repl` | -h only; topic `repl` | `guide` and `guide repl` are not copied (Aside prose) |
| `update` | check and install the newest release | -h only | not run |
| `session list/resume/stop/steer/queue/archive/delete` | agent sessions | leaf options not captured (leaf --help printed root help) | `list` ran. `stop` and `delete` ran on the accidental session only. `stop` on an idle session printed `idle` (exit 0). |
| `settings get`, `settings save-sessions <bool>`, `settings set-default-profile <id>` | CLI settings | leaf options not captured (leaf --help printed root help) | not run |

Exit codes observed:
- `--bogus-flag`: 1.
- `session stop` with no id: 1.
- `host list` with 403: 1.
- A successful `repl` script: 0.
- A `repl` script that throws (syntax error): 0. This is a defect for scripting.
- An unknown word (`bogus-cmd`): treated as a prompt, which started an agent run (exit 0). This is a defect.

Other observations:
- Leaf `--help` after a nested command (for example `account list --help`) printed the 3.3 KB top-level help, not the leaf help. VERIFIED.
- `repl` one-shot output mixes the value and an ANSI-colored status line (`[ok | 10ms]`), so the output is not clean for scripting unless the caller strips it. VERIFIED.
- The CLI prints JSON log lines to stderr for `mcp` (`startup` and `shutdown` events, with `replIdleTimeoutMs` 1800000 and `orphanCheckIntervalMs` 30000). VERIFIED.

## 3. MCP (VERIFIED, run 2; raw files in `mcp-*.json`)

Method: a minimal stdio client in `/tmp` (not in the repo) sent `initialize`, `notifications/initialized`, `tools/list`, `resources/list`, and `prompts/list` to `aside mcp`.

- `initialize`: protocolVersion `2025-06-18` (the server returned the version the client asked for). `serverInfo.name` `aside`, `serverInfo.version` `1.26.1023.1115`. Capabilities: `tools.listChanged` true.
- `resources/list` and `prompts/list`: both return `-32601 Method not found`. Aside's MCP server exposes tools only.
- `tools/list`: four tools. Every tool has `execution.taskSupport` `forbidden` and no `annotations` (no `readOnlyHint` or `destructiveHint`).

| Tool | Purpose (paraphrased) | Input (required in bold) | Group |
|---|---|---|---|
| `repl` | run Playwright-style JavaScript in a persistent sandbox scope against the browser | **`title`**, **`code`**, `profile_id` | browser action |
| `exec` | run Aside's browser agent on logged-in sites; returns `session_id` | **`prompt`**, `session_id`, `profile_id` | agent (not called) |
| `browser_profiles` | list profile ids, labels, and connection status | none | profile state (not called) |
| `memory_search` | semantic search over the user's memory notes; `queries` is 1 to 3 strings, `max_results` 1 to 10 | **`queries`**, `max_results` | personal data (not called) |

Grouping by purpose: one browser-action tool (`repl`), one agent tool (`exec`), one discovery tool (`browser_profiles`), and one personal-data tool (`memory_search`). A browser-only client needs `repl` alone, but `repl` depends on the account and profile setup that `browser_profiles` describes.

The `tools/list` response, without its prose, is about 3.3 KB. With the prose it was 8.8 KB. That prose is the largest part of the tool schema cost, and it is Aside's text, so it is not reproduced here.

## 4. Transport and auth (A5, settled)

VERIFIED (run 2, `lsof`, curl, and `transport-and-auth.txt`):
- The daemon (pid 13450) listens on `127.0.0.1:21420` (TCP). No Unix listening socket for the daemon was found. Run 1 saw one unidentified Unix fd on it.
- The `aside mcp` process (pid 20161) holds one ESTABLISHED TCP connection to `127.0.0.1:21420`. The CLI uses the same loopback TCP port as the browser's network helper (pid 13445, 17 connections at the time of the check). The CLI is not a Unix-socket client.
- `GET /` and `GET /health` return 200 with the same 231-byte JSON status document (service, version `1.26.1023.5`, pid, host, port, ready, status, startedAt, runningSessionCount, semaphore capacity 200). No token was sent. The document contains no user data.
- `Origin: https://example.com` and `Host: evil.test` both return 403. Browser CORS was not tested.

Auth on the CLI-to-daemon link: the CLI binary contains the header names `authorization` and `Authorization`, and `Cookie` and `Set-Cookie`. These are strings, not a capture. INFERRED: the CLI sends an Authorization header with a token to the daemon for protected routes. UNKNOWN: which routes need it (no protected route was identified; `/` and `/health` need nothing). The CLI's credential location was not inspected.

Remote control: `host` says "Remote Control / cloud". `host list` got a 403 from a remote endpoint, so the cloud path exists and was refused for this account. INFERRED: `--host <remote>` goes through a cloud relay. UNKNOWN (not reached).

Unchanged from run 1 (VERIFIED run 1): the browser's main process has a loopback listener on TCP 45103 whose identity is not established. The CDP environment names in the daemon binary are unexplained.

## 5. Permissions and consent (partly settled)

VERIFIED (run 2):
- `--permission ask|guard|full-access` is an option of the root command and of `exec` only. It is not an option of `repl` or `mcp`. The help lists ask, guard, and full-access (ask and guard are the same level; guard is the default). The Settings modes in DOC are Read only, Guard, and Full access, so the CLI has no read-only level. The permission level belongs to agent runs.
- The `repl` tool has no permission parameter. Across about 20 `repl` calls on test pages, I saw no approval prompt and no permission error (fill, click, select, keyboard, download, goto, back, evaluate, wait, screenshot). The UI was not checked for a visible indicator (section 5, Indicator).
- The `repl` calls ran in the signed-in profile (`Profile 0`). A download from a local link saved a 29-byte file, reported through `download.path()` and `suggestedFilename()`.

DOC (note 05, not re-checked): Settings > Agents has Allow, Ask, and Deny tool rules, and Deny wins. Whether those rules apply to `repl` or `exec` through MCP is UNKNOWN. The Deny test needs a settings change, so it was skipped. It is the first test to run when the owner allows a Settings change.

Indicator: settled in run 4 (section 12). The "Agent Tabs" sidebar group appears while a `repl` call holds a tab (VERIFIED). The window-level screenshot was blocked in run 2 and worked in run 4.

Kill switch: `session stop` does not stop a `repl` run (VERIFIED, run 4). No UI stop control was found. NEEDS USER (section 12.3).

## 6. Page representation and token cost (A8, settled)

Format (VERIFIED). The default snapshot is an indented accessibility tree, one node per line:
- The first line is `- title: "..." [url=...]`. Then roles and names, for example `- textbox "Name" [ref=e1] [placeholder="Your name"]: "Probe Name"`, `- checkbox "Agree" [ref=e3] [checked] [focused]`, `- button "Send" [ref=e4]`, `- heading "..." [level=1]`, `- text: "..."`, `- link "..." [ref=e5]`.
- `interactive: true` keeps the interactive nodes plus some structure and text (headings and text nodes stayed in the output), with a `# note:` header line. On GitHub it was 19% smaller (5,351 against 6,595 bytes). On example.com it was larger (329 against 270). Wikipedia and the list page were both capped at about 16 KiB either way.
- Refs are `eN` (for example `e12`). The runtime REPL guide says each new snapshot invalidates earlier refs (read at runtime, not re-checked). Not tested here: whether a ref survives a later snapshot of an unchanged page.
- A call returns `diff` against the immediately preceding snapshot call on the same page: unified-style hunks `@@ -4 +4 @@` with `-`/`+` lines. In a select test with two full snapshots, changing one option produced a 9-line diff (three hunks) instead of the tree. After an interactive snapshot, a full snapshot returned the whole tree as its diff (step 003), so a call after a different-option call returns the full tree.
- Errors: an unknown ref (`e999`) and a stale ref both return `RefStaleError: Ref "eN" is stale ... Take a new snapshot and retry`. The two cases are not distinguished. VERIFIED.

Size cap (VERIFIED): each snapshot is capped at 16 KiB. When the cap applies, the tree ends with `... [snapshot truncated at 16 KiB; narrow it with { interactive: true }, selector or ref]`. There is no pagination and no continuation token. The `interactive` option did not avoid the cap on the two large pages (Wikipedia, and a 492 KB list page). The `selector` option does narrow the scope (one list item was 268 bytes). The `ref` scope was not tested.

Measured sizes (bytes are UTF-8 byte counts of the tree string; tokens estimated as bytes / 4; full table in `snapshot-sizes.txt`):

| Page | Raw HTML (run 1 unless noted) | Snapshot (tree) | Interactive | PNG screenshot | Tree / raw | Est. tokens (tree) |
|---|---|---|---|---|---|---|
| example.com | 577 | 270 (3 lines) | 329 | 199,568 | 47% | ~68 |
| Wikipedia, Web browser | 415,391 | 16,469, capped | 16,466, capped | 845,839 | 4.0% | ~4,117 |
| github.com/tonibergholm/yaac | 245,228 | 6,595 (151 newlines) | 5,351 | 396,659 | 2.7% | ~1,649 |
| local list page (4,000 items; raw size of the file served in this run) | 492,816 | 16,465, capped | 16,453, capped | not measured | 3.3% | ~4,116 |

- The local form page's whole snapshot result, including the open-tab line, was 542 bytes.
- The image path: `display(png)` re-encodes to JPEG, resized to 2000 px wide (a 2880 x 1800 page became 2000 x 1250, 112,872 base64 characters, about 84.6 KB). The annotated screenshot of the same page was 73,768 base64 characters. An image costs about 3,333 tokens at the usual (w x h) / 750 approximation. INFERRED (not measured).
- So for text-heavy pages the tree is 1 to 4 thousand tokens, against a screenshot of about 3 thousand tokens. Raw HTML for the two text-heavy run-1 baselines is about 61 thousand tokens (GitHub) and 104 thousand (Wikipedia). example.com is about 68 tokens for its tree and about 144 for its raw HTML. The 16 KiB cap is the main reason the cost stays bounded.

Not verified: `page.content()` and `page.evaluate()` costs (the REPL guide says to avoid them for reading; not measured).

## 7. Actions in the REPL (VERIFIED on test pages)

All of these ran on test pages in the signed-in profile:
- `openTab(url)`: opened a tab, set `page`, and added it to `tabs`. Output line: `Opened a new tab and set it active: tabs[n], page -> <title> (<url>)`.
- `locator('eN').fill(text)`, `.click()`, `.selectOption(value)`: worked by ref.
- `page.keyboard.type(text)`, `page.keyboard.press('Tab')`: worked.
- `page.evaluate(fn)`: returned a value.
- `page.waitForSelector(sel, {timeout})`: a timeout reported `waitForSelector: timed out after 5000ms waiting for "#status" to be visible`, with `isError` true.
- `page.waitForEvent('download', {timeout})` then `download.path()`, `download.suggestedFilename()`: worked.
- `page.goto(url)`, `page.goBack()`, `page.url()`: worked.
- `page.screenshot()`, `annotatedScreenshot(page)`, `display(buffer)`: worked.
- `closeTab(tab)`: closed a tab and updated `tabs`. Closing all seven of my tabs worked.
- The download saved `probe.txt` (29 bytes) to the user's `~/Downloads`. It was my file (same size and content as my test string), and I removed it (section 0). Aside's download history in Profile 0 probably holds an entry (INFERRED).

Behaviour notes:
- The REPL keeps one scope per MCP process. A redeclared `let` fails with `Identifier 'x' has already been declared`. VERIFIED.
- A warning appears when many tabs are open: `[warning] 7 tabs are open. Close tabs you do not need as you work.` VERIFIED.
- Uncaught errors return the message plus a stack trace that includes internal file paths in the app bundle. VERIFIED. A defect to avoid in our tool output.
- `fs` writes outside the project and session roots are refused. VERIFIED.
- Screenshots passed to `display()` are also saved under the Aside session directory. The path is not read in this run.

Not tested: click on a covered element, cookies, PDF, network inspection, dialog handling, the 120-second timeout, and the agent's `--permission` levels (agent runs are not allowed in this run).

## 8. REPL API surface (names from the `repl` tool and the `aside repl` guide, read at runtime; names only)

Globals: `page` (the last opened tab), `tabs` (tabs opened in this session), `getTabs()` (named in the `repl` help example; not run), `listBrowserTabs()`, `attachBrowserTab(targetId)`, `attachActiveBrowserTab()`, `getTabByTargetId(id)`, `openTab(url)`, `closeTab(tab)`, `snapshot(page, options)`, `annotatedScreenshot(page)`, `display(data)`, `fs`, `path`, `Buffer`, `sleep(ms)`, `fetch(url)` (carries the user's cookies), `pwd`, `aside` (account settings, sessions, projects, routines), and site-skill globals loaded from `aside skills` (for example `slack`, `gmail`).

Page methods used in this run: `locator(ref).click/fill/selectOption`, `keyboard.type/press`, `evaluate`, `waitForSelector`, `waitForEvent`, `goto`, `goBack`, `url`, `screenshot`. Listed in the guide but not run: `pdf`, `locator.screenshot`, `title`, `content`.

Snapshot options (names from the guide): `interactive`, `showHidden`, `ref`, `selector` (a CSS selector, not an ARIA role name).

The guide forbids `page.context().newPage()` and `page.close()`, and says to use `openTab` and `closeTab`.

Token-efficient parts, and why they work:
- Refs (`eN`) replace CSS selectors in every action. They are short and stable within one snapshot.
- Diff after an action returns only the changed lines when the preceding snapshot call used the same options (step 009). After a call with different options the full tree came back (step 003).
- The tree omits layout, styling, and markup. It keeps roles, names, values, and states.
- The 16 KiB cap bounds each read.
- `selector` narrows the read (268 bytes for one list item). `interactive` cut GitHub by 19% and did not help the other pages. `ref` was not tested.
- Images are resized before display.

## 9. Settles and still unknown (against note 05)

Settled in this run (VERIFIED):
- A5 (daemon transport): settled. Loopback TCP 127.0.0.1:21420, shared with the browser helper. Not a Unix socket. Status document needs no credential. Foreign Origin and Host get 403.
- A6 (`tools/list`): settled. Four tools: `repl`, `exec`, `browser_profiles`, `memory_search`. No annotations. No resources, no prompts. Note 05's claim of a single `repl` tool is refuted. Note 05's Playwright-style page API names are confirmed in the REPL (section 8).
- A8 (snapshot size): settled. Format, cap, and sizes are in section 6.
- CLI command tree and exit codes: settled (section 2), with the two defects (unknown word as prompt, exit 0 on script error).
- Carried from run 1, not re-checked: the Chromium major version 154 (INFERRED), the Node.js daemon (INFERRED), and the signing and entitlements (VERIFIED, run 1). The daemon status version differs from the plist version (VERIFIED).

Settled in run 4 (section 12): the window indicator during a `repl` call (VERIFIED), and the accidental run's cost of 3 credits (section 12.6).

Partly settled:
- Indicator and kill switch: the window indicator is VERIFIED. `session stop` does not stop a `repl` run (VERIFIED). No UI stop control was found, and the owner check is NEEDS USER (section 12.3).
- Permission model for MCP and CLI calls: the `repl` path showed no prompts. The Deny-rule test (run 3, section 11) showed that Settings tool rules do not apply to external calls.
- Daemon auth: the status route needs no credential. Auth on other routes is UNKNOWN.
- Remote control: the cloud path exists (403 for listing hosts). UNKNOWN beyond that.

Still unknown:
- Indicator outside the Aside window (OS notifications, Dock badge): not checked.
- Kill switch for an external client: no UI control found; NEEDS USER (section 12.3).
- What brings Aside to the front during a call (section 12.4).
- Whether `closeTab` really closes the tab: the runtime says "closed", the sidebar keeps the tab (section 12.2).
- Audit log of agent or REPL actions.
- Whether Settings > Agents Deny rules apply to `repl` and `exec` through MCP.
- Session id format and expiry. Note 05 (DOC) lists `--session <id>`, which is not in the 1.26 help; `session resume <id>` is the continuation command here.
- Whether the REPL's `fetch` restricts cross-origin cookies (documented as cookie-bearing).
- Whether an unauthenticated-looking route beyond `/` and `/health` exists.
- Tab lifecycle at process exit.

## 10. Verdict and recommendations for yaac

Verdict: Aside's agent surface is four MCP tools, a stdio server, and one loopback TCP daemon port that serves an unauthenticated status document. Its REPL-first design gives compact reads (diff, refs, a 16 KiB cap) and fine-grained actions. It applies no visible per-action permission to the REPL path on test pages, and it has two CLI defects that matter for scripting. Its status route answered an unauthenticated request from this user's shell (VERIFIED). INFERRED: any local process can make the same request. Permissions, the indicator, and the kill switch are the open parts.

Concrete recommendations for the yaac agent interface (inferred from the observations above; each is labelled):

Tool set
- The four Aside tools covered every task I tried, but only `repl` was called. INFERRED for yaac: ship a typed tool set of about 9 to 10 tools, not 17 (note 05 section 5.3 was a 17-tool list), with actions merged: one `act(tab, action, ref, value)` instead of four separate tools, and `navigate` returning when the page is interactive. The runtime guide says `openTab` and `click` wait for interactivity (text read at runtime, not re-checked here), so a separate `wait` is needed only for selectors and text.
- Do not ship an arbitrary-code tool as the default or only browsing surface. VERIFIED basis: the `repl` tool performed fill, click, download, and navigation with no prompt that I could see. INFERRED: an arbitrary-code tool bypasses per-action permission checks unless every Page method is wrapped in the core's policy. If yaac ships one, it must go through the same grant checks; otherwise it stays out of v1.
- Keep `memory`-type tools out of the browser server. Aside's `memory_search` sits in the same MCP server as the browser tools (VERIFIED). INFERRED: it widens the blast radius of any prompt injection.
- Tool schema cost: Aside's 4-tool schema is about 8.8 KB with prose, of which most is the code tool's description. INFERRED: keep descriptions short and factual; do not put policy prose into tool descriptions (the core enforces policy).

Snapshot format
- Copy: the indented accessibility tree with short refs, the `selector` scope (VERIFIED to narrow the read), and the diff after actions (VERIFIED when the preceding snapshot used the same options). Treat the `interactive` filter as a modest size control: 19% smaller on GitHub, larger on example.com, no change on the capped pages (VERIFIED). `ref` scope: not tested (section 6).
- Change: put an explicit continuation or truncation field in the result instead of a hard text marker. Aside cuts at 16 KiB with no way to page. INFERRED: yaac should return `truncated: true` with a `next` cursor or a suggested narrower scope.
- Change: distinguish "ref not found" from "ref stale". Aside returns `RefStaleError` for both. VERIFIED.
- Keep the 16 KiB default (or a similar bound). It makes a read cost 1 to 4 thousand tokens on text-heavy pages.
- Screenshots: return a file path in the Space's artifact folder, and send an image to the model only on request, resized. VERIFIED that `display` resizes. INFERRED that the file-path route is cheaper for logs.

Permission model
- Enforce grants, origins, and confirmations in the core (note 05 R3 to R5). Aside's observed REPL path had no visible check, and its permission flag exists only for agent runs (section 5). That is the gap to avoid.
- Prompt on the first action on a new origin in a logged-in Space, and on every privileged action (evaluate, download, cookies). VERIFIED basis: nothing in Aside's REPL path prompted on a download or evaluate.
- Error results must not include stack traces or internal paths. VERIFIED that Aside's errors do.

Transport
- Do not use a loopback TCP port with a no-credential status route. Aside's daemon shows this design (VERIFIED). Use a Unix domain socket at mode 0600 in a 0700 directory with a peer-uid check (note 05 R1). If a status command is needed, put it on the socket behind the same peer check.
- The CLI and the MCP server should be thin clients of one core process (VERIFIED: Aside's CLI is a client of the daemon).

CLI behaviour
- Unknown words must be errors, not prompts. VERIFIED defect in Aside: `bogus-cmd` started an agent run.
- A failed script must exit non-zero. VERIFIED defect: a syntax error exited 0.
- Leaf `--help` must print leaf help. VERIFIED defect: it printed the top-level help.
- Machine output should be clean: a value on stdout and status on stderr, with no ANSI in piped output. VERIFIED: Aside mixes the two on stdout.

Indicator and kill switch (run 4, section 12)
- Indicator: Aside shows an "Agent Tabs" sidebar group and a REPL session panel while a call runs (VERIFIED). INFERRED for yaac (note 05 R6): show an indicator in the Space's UI for every active external-control session, and clear it only when the core confirms the session has ended, not when the script returns.
- Kill switch: Aside's `session stop` does not stop a `repl` run (VERIFIED). INFERRED for yaac (note 05 R8): a stop must abort the running script, close the tab the session owns, and end the session, and the core must confirm all three. No UI stop control was found (NEEDS USER, section 12.3).
- Tab cleanup: Aside's `closeTab` returned "closed" while the tab stayed in the sidebar (VERIFIED). Do not treat a script's return value as state; check the core's own record.
- Focus: an external call brought Aside to the front and replaced the active view (VERIFIED, run 4). INFERRED for yaac: do not switch the user's view on an external call; if a call must surface a tab, make that an explicit setting.
- Screen Recording was not blocked in run 4. The window was captured by ID from a Swift CoreGraphics helper (section 12).

Before freezing the yaac tool schema, run these: the owner's kill-switch check (section 12.3, NEEDS USER), and a check of whether the CLI's Authorization header is required for any route beyond `/` and `/health`. Done since run 2: the Deny-rule test (run 3) and the indicator screenshot (run 4). The accidental run's cost is settled: 3 credits (section 12.6).

## Verdict on this run

Settled: CLI command tree, exit codes, the MCP tool list, transport (loopback TCP 21420), the daemon's Origin and Host checks, the snapshot format and sizes, the 16 KiB cap, and the REPL action surface on test pages.

Not settled: approval and indicator behaviour, the kill switch, the audit log, and what the accidental agent run cost. These need the owner's approval for a Settings change, a window ID, or a check of Aside's usage history.

## 11. Deny-rule test (run 3, 2026-10-23)

Setup: the owner added one rule in Settings > Agents > Tool permissions: Deny, tool `repl` (the Allow and Ask lists were empty; the rule is visible in a screenshot the owner supplied). No other setting was changed.

Test, from the main session, using only a tab it opened and closed itself on https://example.com:

- MCP: `tools/call` on `repl` through a stdio client of `aside mcp`. The call ran: `isError: false`, the snapshot of example.com came back, the tab was closed. VERIFIED.
- CLI: `aside repl "<same code>"`. It ran and printed the snapshot (`ok`, 365 ms). VERIFIED.
- `repl` return values are not printed; only `console.log` and `display` output reaches the client. A first attempt that used `return` showed an empty result and looked like a block. A control run with `console.log("hello")` printed `hello`, which ruled that out. VERIFIED.

Result: with a Deny rule for `repl` present in Settings, `repl` called through `aside mcp` and `aside repl` still executed. VERIFIED. INFERRED: the Settings tool rules govern Aside's own agent runs, not calls made by an external MCP or CLI client. Not tested: whether the rule applies to `repl` inside an `aside exec` agent run (that would start a billable agent session), and whether a rule saved in a different form (for example one tied to the MCP server) would apply.

Consequence for yaac: an external client path can sit outside the permission rules. Our core must enforce grants on the same path for the UI, the CLI and MCP (note 05 R3 to R5), with no unguarded route.

Owner action: remove the Deny rule for `repl` if it is still set.

## 12. Indicator and kill switch (run 4, 2026-10-23)

Method. One `aside repl` call opened one tab of its own (example.com), held it for 18 seconds with `sleep`, and closed it in a `finally` block. The Aside window was captured by its ID (1281) with `screencapture -l`. The ID came from a small Swift script that calls CoreGraphics and keeps only windows whose owner is "Aside" and whose layer is 0. Screen Recording did not block the capture. The run also used `lsappinfo front` and `aside session list`. A second probe ran a `repl` with no tab and a 60-second sleep, then called `aside session stop` on that session, which was the only session in the list. Nothing was clicked and no keys were sent. The raw timeline is in `docs/research/06-aside-observed/run4-indicator-and-kill-switch.txt`. It has no window titles. Screenshots stayed under `/tmp/aside-indicator/run4/` and are not committed, because they show the owner's private tabs and chats.

### 12.1 Indicator during the call

- VERIFIED: about 4 seconds after the call began, the sidebar Tabs list held a group labelled "Agent Tabs" with a count badge "1" and a disclosure chevron. The group sat between "New tab" and the regular tabs and held the tab the call opened, titled "Example Domain". The Settings view was still the active view at that point.
- VERIFIED: about 13 seconds after the call began, the example tab was the active view. The window showed a right-hand panel titled "Aside CLI REPL", with a title dropdown, compose, expand and close icons, an empty chat area, the input "Ask AI a task, @ for context", a model picker "GPT-6.1 Sol" with "High", and the pill "Connect my AI subscription" above the input.
- VERIFIED: no banner, toast, tab colour change, badge on the tab, or stop button was visible in any capture. A small orange icon with a caret sits at the bottom left of the input. I did not read its label.
- VERIFIED: the probe with no tab showed the same right-hand panel but no "Agent Tabs" group, because it opened no tab.
- NOT CHECKED: OS notifications or Dock badges outside the window.

### 12.2 Persistence after the call

- VERIFIED: the "Agent Tabs" group disappeared when the call ended (the captures at +0 s and +10 s). The example tab stayed in the ordinary Tabs list, below "Settings > Plan & Usage", and it stayed the active view. Captures at about 90 seconds and about 3 minutes later showed the same state.
- VERIFIED: the call printed `[system] last tab closed` and `closed`. The runtime therefore reports the tab closed, while the sidebar still lists it. I could not tell whether the browser's own tab was closed, because listing tabs is forbidden. UNKNOWN: whether the tab is still live. The owner should close it (section 12.3, step 1).
- VERIFIED: the right-hand "Aside CLI REPL" panel stayed open after the call and was still empty, although `aside session list` showed no sessions.
- VERIFIED: the sidebar "Chats" section showed only "New Chat" during and after the call. No session item appeared there.
- VERIFIED: `aside session list` showed one entry during a call, with status idle, type ephemeral, and the title "Aside CLI REPL". After the call the list was empty. So the session record exists only while the call runs.
- DOC-consistent (help text only): `aside settings --help` says CLI runs stay off the chat list unless `save-sessions` is true. I did not record the value of the setting.

### 12.3 Kill switch

- VERIFIED: `aside session stop <id>` on a live `repl` session printed `idle` and exited 0. The `repl` kept running. About 5 seconds later it was still alive. It finished on its own after 60 seconds, printing `finished-after-60s` with `ok | 60015ms`. So `session stop` is not a kill switch for `repl`. The agent-run case was not tested, because agent runs are not allowed.
- VERIFIED: no stop control appeared in the REPL panel during the active call. The orange icon at the bottom left of the input is the only unlabelled control. UNKNOWN: its function.
- VERIFIED (key names only, values not recorded): `aside settings get` prints four keys: `accountId`, `defaultProfileId`, `defaultProfileSelector`, `saveSessions`. None controls external access.
- VERIFIED (run 3): the Settings tool rule Deny for `repl` did not stop `repl` calls from `aside mcp` or `aside repl`. So the Agents rules are not a kill switch for external clients.
- The sidebar Settings list shows these labels: Agents, Plugins & MCPs, Developers, Security & Privacy, Mini popup. I did not open them. UNKNOWN: whether any of them has an external-control switch.

NEEDS USER. The owner can do these without the CLI. None needs a Settings change, and the owner should change nothing.
1. Close the tab "Example Domain" in the sidebar's Tabs list. It is my test tab. It is the only example.com tab, and it was absent from the baseline. If the "Aside CLI REPL" panel is still open, close it with its X at the top right.
2. Kill-switch test, about 2 minutes. In a terminal, run `aside repl "await sleep(60000); console.log('done')"`. This opens no tab. While it runs, hover over the controls at the top right of the "Aside CLI REPL" panel and over the orange icon at the bottom left of the input, and note their labels. Do not click them yet. Then click the panel's X and check whether the terminal command ends early. Report the labels, and whether the command kept running after the X.
3. Open Settings > Developers, Settings > Agents and Settings > Security & Privacy. Report any toggle or label about external clients, MCP, the CLI or remote control, with its state. Change nothing.

### 12.4 Focus

- VERIFIED: before the call, the front application was "T3 Code (Nightly)", the app that hosted my shell. It was still in front at +4 s. At +13 s the front application was "Aside", and it stayed in front in the later captures.
- VERIFIED: the Aside window was already on screen, so the call did not restore it from hidden. It brought Aside to the front and made the example tab the active view, which replaced the owner's Settings view. The Settings view was not restored afterwards.
- UNKNOWN: the trigger. The change happened between +4 s and +13 s. The repl printed the snapshot at once, so the activation could come from `openTab`, from the snapshot, or from the panel opening. The probe with no tab did not record the front application.

### 12.5 Other items

- VERIFIED: no confirmation prompt for any `repl` action in these runs. The "Connect my AI subscription" pill is an account action, not an approval. The earlier runs (sections 5 and 7) also showed no prompt for download or evaluate.
- VERIFIED: no per-site grant or permission prompt appeared in any capture. Settings > Security & Privacy was not opened, so UNKNOWN for per-site grants.
- VERIFIED: the capture size changed between captures (1246 by 1076 pixels, then 1312 by 1142) while the CoreGraphics bounds stayed at 1200 by 1030. The cause is not established.

### 12.6 Credits for the accidental run (section 0)

- VERIFIED: Settings > Plan & Usage lists the credit rows for October 2026. One row is "bogus-cmd", model aside/gpt-6.1-sol, 3 credits, Oct 23 10:26 AM local time. A second row, "Generated task title", model aside/gpt-6-luna, is 0 credits. So the accidental run cost 3 credits. I did not re-check the usage list after this run.
- VERIFIED: no "Agent Tabs" group was visible in the baseline capture at 13:07, so the accidental run left no visible tab group at that point. Whether it opened a tab earlier is still UNKNOWN.

### 12.7 Verdict for run 4

- The window indicator is settled (VERIFIED): a sidebar "Agent Tabs" group with a count, a REPL session panel in the window, and the tab in the group, all during the call. Once the call ends, the group disappears, but the tab and the panel stay.
- The kill switch is not found. `session stop` does not stop a `repl` run (VERIFIED). No UI stop control was found (VERIFIED). The owner check in section 12.3 is required (NEEDS USER).
- Design consequences for yaac are in section 10. The most important are that closing must be confirmed against the core's own state, and that an external call must not change the user's active view.

