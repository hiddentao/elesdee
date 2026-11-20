# Docs consistency audit: Funnel, delete, phone package, accessibility

Date: 20 November 2026
Branch: `hiddentao/round-2-mockups`, head `872b704`
Method: read `AGENTS.md`, `.dev/SPEC.md`, `.dev/api.md`, `.dev/phone-shell.md`, `.dev/providers-configs.md` and every file in `docs/`, then checked each statement against the code. No product code was edited. The Rust and JavaScript suites were not run: this audit compares documents and code only.

Scope: the behaviors changed on 20 November 2026 — Tailscale Funnel, deleting a session with a possibly-public address, the computer-scoped phone UI package, the desktop and phone accessibility work, and the `AGENTS.md` phase summary.

## Summary

| # | Type | Severity | Where |
| --- | --- | --- | --- |
| 1 | Wrong code | High | A demo-server registration can drop the only record of a public Funnel mapping (`core/src/sessions.rs:904`) |
| 2 | Wrong doc | Medium | `SPEC.md:571` says a refused turn-off shows the reason and Stop can be retried on every path; only the Stop path does (`api.md:389` is correct) |
| 3 | Wrong doc | Low | `api.md:388` says an enable is refused when the session starts pausing; during the pause grace it succeeds and is turned off later |
| 4 | Wrong doc | Medium | `AGENTS.md:34` still lists "searching GitHub from the launch form" as not built |
| 5 | Wrong doc | Low | `AGENTS.md:31` omits Delete from the Funnel turn-off list |
| 6 | Wrong doc | Medium | `docs/troubleshooting.md:39` says only Claude Code has a permission card |
| 7 | Wrong doc | Medium | `docs/agents.md:57-68` vendor table lists one client per vendor |
| 8 | Wrong doc | Low | `docs/agents.md:107` says OpenCode 2 is the default client for a custom provider |
| 9 | Wrong doc | Medium | `docs/getting-started.md:48-52` add-provider steps name a Google key and skip the vendor choice |
| 10 | Doc/code conflict | Medium | `docs/settings.md:16` says paused work does not hold the computer awake; the code holds it awake |
| 11 | Wrong doc | Low | `SPEC.md:152-153` omits Reopen and the session-header menu from the session menu |
| 12 | Wrong doc | Low | `SPEC.md:586` says the phone keeps two builds; it keeps three |
| 13 | Wrong doc | Low | `docs/phone.md:19` describes the retained previous build as "for the next attempt" |
| 14 | Not implemented | Medium | `phone-shell.md:496` says the shell blocks navigation away and new windows; no handler exists |
| 15 | Stale doc | Low | `phone-shell.md:656` says the UI package methods return `unsupported` |
| 16 | Imprecise doc | Low | `api.md:397` says the phone receives only local and tailnet addresses; the record also carries the public address |

## Findings

### 1. A demo-server registration can drop the only public-address record (wrong code, high)

- Doc: `.dev/SPEC.md:572` — "The only record of the public port is never dropped while the daemon may still serve it." See also `.dev/api.md:390` ("A session that still holds a public address is never removed") and `.dev/SPEC.md:571`.
- Code: `core/src/sessions.rs:904-916`. When the session's demo list already holds eight records, a new registration removes the first stopped record to make room. It does not check `funnel_address`, and it does not turn the mapping off first.
- Reachable state: a pause, close or app restart that Tailscale refuses leaves `funnelAddress` on a record whose state is then `stopped` (`core/src/sessions.rs:1925-1932`, `core/src/state.rs:2339-2373`, `core/src/recovery.rs:63-81`). The session stays active (paused) and can resume. The agent then registers servers; when the list is full the public record can be removed.
- Consequence: the Tailscale daemon may still serve that port while the app has no record of it. `public_demo_servers` (`core/src/state.rs:2313-2329`) cannot find it, so enabling another server will not turn it off, and two ports can be public at once.
- Correction: never prune a record with `funnel_address` set. Refuse the new registration with `DemoFunnelStillPublic` (or turn the mapping off first and only prune after the client accepts). Add a test with eight stopped records, one public, and a ninth registration.

### 2. A refused turn-off shows no reason and offers no retry after close or pause (wrong doc, medium)

- Doc: `.dev/SPEC.md:571` — "A refused turn-off keeps the public address on the record... The panel shows the reason and Stop can be retried."
- Code:
  - Stop: `core/src/state.rs:2379-2407` returns `DemoFailure::FunnelUnavailable`; `core/src/api.rs:1196-1203` maps it to 422. The server stays active, so the panel still shows "Stop public" and the error (`app/src/components/SessionSidePanel.tsx:720-724, 788-864`).
  - Close: `core/src/api.rs:2829` and `core/src/state.rs:2593-2603` call `disable_demo_funnels`, which only logs a refusal (`core/src/state.rs:2352-2370`). The session then closes.
  - Pause: `core/src/api.rs:3714-3756` calls the same cleanup; the session is then marked paused and every server becomes `stopped`.
- What the user sees after a pause: the server row has state `stopped`, the public row and the Stop button are rendered only for active servers (`app/src/components/SessionSidePanel.tsx:804-864`), so the retained public address and the refusal reason are both hidden.
- `.dev/api.md:389` describes the actual behavior: "close, pause and startup log the refusal and keep the address on the record for a later retry."
- Correction: either narrow `SPEC.md:571` to the Stop action and say close and pause keep the address for the next app start or a Delete attempt, or change the code to carry the cleanup reason into the paused note and to show the retained public address with a retry action on a paused session. The first option matches the rest of the code; the second matches the spec's intent.

### 3. An enable during the pause grace is not refused (wrong doc, low)

- Doc: `.dev/api.md:388` — "A session that starts closing or pausing while the command runs turns Funnel off again and refuses the request."
- Code: `core/src/state.rs:2256-2272` re-checks the session after the client command, but only for `status != Active`, `entry.is_closing()` and the server still active. `run_pause` does not start closing until the grace ends (`core/src/api.rs:3742-3756`). During the grace the session is still active and not closing, so an enable returns success and records the address; the cleanup at `core/src/api.rs:3756` then turns it off.
- Correction: say "A session that starts closing while the command runs ... is refused; a session that starts pausing has Funnel turned off again when the pause finishes." Or set a pausing flag at the start of `run_pause` and check it in `enable_funnel`.

### 4. `AGENTS.md` still lists launch-form GitHub search as not built (wrong doc, medium)

- Doc: `AGENTS.md:34` — "Not built yet: client discovery and installation on Windows and Linux, searching GitHub from the launch form, ..."
- Code: the launch form searches the connected account's repositories — `app/src/components/NewSessionDialog.tsx:449-476` calls `github.listRepos`, and the e2e test `e2e/07-launch.spec.ts:161-209` starts a session from a repository that is not on the computer. `docs/sessions.md:30` documents the feature.
- Correction: remove "searching GitHub from the launch form" from the not-built list.

### 5. `AGENTS.md` omits Delete from the Funnel lifecycle (wrong doc, low)

- Doc: `AGENTS.md:31` — "Stop, close, pause and an app restart turn Funnel off."
- Code: Delete also turns every public address off first and refuses with 422 while one remains (`core/src/api.rs:3859-3941`, `core/src/state.rs:2624-2643`). The spec and the public docs cover this (`SPEC.md:572`, `api.md:390`, `docs/sessions.md:164`).
- Correction: add "Delete turns every public address off before it removes anything and refuses while one remains" to the phase summary.

### 6. The troubleshooting page still says only Claude Code has permission cards (wrong public doc, medium)

- Doc: `docs/troubleshooting.md:37-39` — "The card belongs to Claude Code sessions. Codex, OpenCode 2 and Antigravity keep their prompts in their terminals in the current build."
- Code: Codex raises approval requests through its app server (`core/src/codex.rs:190-196`) and OpenCode 2 through its permission list (`core/src/opencode.rs:83-140`). The app shows cards for both. `docs/permissions.md:9` and `:28-32` and `SPEC.md:250-251` say so.
- Correction: replace the sentence with "Claude Code, Codex and OpenCode 2 use cards; Antigravity keeps its prompts in its terminal." `docs/index.md:20` ("Approve or deny Claude Code permission requests from a card") should also name all three clients.

### 7. The vendor table in `docs/agents.md` lists one client per vendor (wrong public doc, medium)

- Doc: `docs/agents.md:57-68` — Anthropic: Claude Code; OpenAI: Codex; DeepSeek: Claude Code; Kimi and Z.ai: Claude Code; OpenRouter: OpenCode 2; Ollama and LM Studio: Claude Code.
- Code: `core/src/providers.rs:18-118` supports Anthropic on Claude Code and OpenCode 2; OpenAI on Codex and OpenCode 2; DeepSeek on Claude Code, Codex and OpenCode 2; Kimi and Z.ai on Claude Code and OpenCode 2; OpenRouter on OpenCode 2 and Codex; Ollama and LM Studio on Claude Code and OpenCode 2. `SPEC.md:371-381` and `.dev/providers-configs.md:116-126` already carry the full table.
- Correction: make the public table match `providers.rs`. If a single-cell layout is wanted, write the client list in the cell, as the Custom row already does.

### 8. The custom-provider default client is not OpenCode 2 (wrong public doc, low)

- Doc: `docs/agents.md:107` — "OpenCode 2 is the default client."
- Code: the custom provider form defaults to the OpenAI style (`app/src/components/ProviderDialog.tsx:108-109`), whose clients are `[Codex, OpenCode 2]` (`core/src/providers.rs:18-19, 128-136`). The config form picks the first supported client (`app/src/components/AiConfigDialog.tsx:202-207`), so a new OpenAI-style custom provider selects Codex; an Anthropic-style provider selects Claude Code.
- Correction: say "The form picks the first client the chosen style supports" or remove the sentence.

### 9. The get-started provider steps do not match the form (wrong public doc, medium)

- Doc: `docs/getting-started.md:46-52` — "Choose Add provider. Enter a name and a Google API key. Save."
- Code: the form starts with the vendor list (first vendor: Anthropic) and the sign-in method, and only later shows the key or server fields (`app/src/components/ProviderDialog.tsx:106-117`). `docs/agents.md:71-77` gives the correct steps.
- Correction: "Choose the vendor and the sign-in method, then enter a name and the key, server URL or sign-in that the vendor needs."

### 10. Paused agent sessions hold the computer awake (doc/code conflict, medium)

- Doc: `docs/settings.md:16` — "Terminal sessions, closed sessions and paused work do not hold it awake." `SPEC.md:73` says the computer is kept awake "while any agent session is running".
- Code: `app/src/App.tsx:230-232` defines `agentRunning` as any session with `status === 'active'` and `kind === 'agent'`, with no work-state filter, and `:339` sends `settings.keepAwake && agentRunning` to the shell. A paused session (and a reported-done session) is active, so it holds the computer awake.
- Correction: either exclude `paused` (and consider `done`) in `agentRunning`, or change `docs/settings.md:16` and `SPEC.md:73` to say any active agent session holds it awake, including paused work. The doc and the spec currently agree with each other against the code, so the code is the more likely fault.

### 11. `SPEC.md` omits Reopen and the header menu from the session menu (wrong doc, low)

- Doc: `.dev/SPEC.md:152-153` — "Right-click a session in the sidebar, or its tab, to open a menu of actions. The menu holds Rename, Switch AI config, Move to project, and Close or Delete."
- Code: the menu adds Reopen for a closed session and Copy folder path for a session without a branch (`app/src/components/SessionActionsMenu.tsx:60-107`), and it also opens from the session-header button (`app/src/components/SessionHeader.tsx:105-115`). `docs/sessions.md:168-176` documents all of this.
- Correction: add "or the menu button in the session header", and add Reopen to the list of menu items.

### 12. The phone keeps three builds, not two (wrong doc, low)

- Doc: `.dev/SPEC.md:586` — "The phone keeps the build on screen and the current build".
- Code: it keeps the current build, the build it replaces and the build on screen (`phone-shell/src-tauri/src/ui.rs:339-365, 792-813`), which is what makes the second update safe. `.dev/phone-shell.md:438` and `:958` already name all three.
- Correction: name all three builds in `SPEC.md:586`.

### 13. The public phone doc misdescribes the kept build (wrong public doc, low)

- Doc: `docs/phone.md:19` — "It checks the download before it uses it and keeps the previous copy for the next attempt."
- Code: the previous build is kept so a page that is open during an update, or during a second update, keeps loading its scripts and styles (`phone-shell/src-tauri/src/ui.rs:339-365, 792-813`; `SPEC.md:586`). The next attempt does not use it; a new download replaces it.
- Correction: "It checks the download before it uses it and keeps the current build, the one it replaces and the build on screen, so an open screen keeps working through an update."

### 14. The shell does not block navigation or new windows (not implemented, medium)

- Doc: `.dev/phone-shell.md:496` — "The shell blocks navigation away from the shell origin and the package origin, and denies new windows."
- Code: `phone-shell/src-tauri/src/lib.rs:27-75` registers the protocol, plugins, state and commands, but sets no `on_navigation` handler and no new-window handler. `phone-shell/src-tauri/tauri.conf.json` has no window navigation setting. The CSP applies to the package's own resources, not to a top-level navigation.
- Correction: either implement the guard (`WebviewWindowBuilder::on_navigation` allowing only the shell and package origins, plus `on_new_window` returning deny) or mark the sentence as not built in this shell.

### 15. The UI package methods are described as unsupported (stale doc, low)

- Doc: `.dev/phone-shell.md:656` — "The UI package methods return `unsupported` until their step."
- Code: `ui.manifest` and `ui.download` are dispatched and served (`core/src/link/app.rs:310, 377, 573`; `core/src/link/ui.rs`), and step 5 is marked implemented in the same document (`:741`).
- Correction: change the sentence to past tense ("returned `unsupported` before step 5") or remove it.

### 16. The phone receives the public address as well (imprecise doc, low)

- Doc: `.dev/api.md:397` — "The phone receives only active ports, with each record's local and tailnet addresses." `docs/phone.md:81` says the phone "does not show the public address".
- Code: `demoServers.list` returns the serialized `DemoServer`, which includes `funnelAddress` when set (`core/src/demo.rs:46-50`). Neither phone surface renders it, so the user-facing statement is true.
- Correction: say "each record's local and tailnet addresses; a public address is included but the phone does not show or offer it", or strip `funnelAddress` from the link response.

## Facts that appear in two documents and disagree

None remain among the documents changed in this pass. Each fact below now reads the same way everywhere it appears:

| Fact | Documents that now agree |
| --- | --- |
| A refused Funnel turn-off keeps the address; Stop shows the reason and can be retried, and a stopped server shows the retained address with the same action | `SPEC.md`, `.dev/api.md`, `docs/session-tools.md`, `AGENTS.md` |
| The session menu | `SPEC.md` and `docs/sessions.md` both name Reopen and the menu button in the session header |
| Clients per vendor | `docs/agents.md` now carries the full list from `core/src/providers.rs`, as `SPEC.md` and `.dev/providers-configs.md` already did |
| Add-provider steps | `docs/getting-started.md` and `docs/agents.md` both start with the vendor and the sign-in method |
| GitHub launch search | `AGENTS.md` and `docs/sessions.md` both treat it as built |
| Permission cards | `docs/troubleshooting.md`, `docs/index.md`, `docs/permissions.md` and `SPEC.md` all name Claude Code, Codex and OpenCode 2 |
| Builds kept on the phone | `SPEC.md`, `.dev/phone-shell.md` and `docs/phone.md` name the build on screen, the current build and the build it replaces, and give the same reason for keeping them |
| Keep-awake while paused | `docs/settings.md` and `SPEC.md` say an active agent session holds the computer awake, including a paused one, and the code agrees |

## Public docs that name internal behavior

None remain. These pages now describe what a user sees:

- `docs/session-tools.md` says the tools and skills reach Claude Code, Codex and OpenCode 2 sessions and leave the repository and global settings alone, without naming the server or the per-client launch files.
- `docs/agents.md` says the provider's settings and key reach the client and the session's temporary settings are removed, without naming the child environment or the private home folder.
- `docs/phone.md` says the downloaded interface uses the phone app's connection, without naming the local API or the transport selection.
- `docs/troubleshooting.md` no longer carries development instructions; `AGENTS.md` already covers them under "Running the app".
- `docs/sessions.md` says the right repository is chosen even when two checkouts share a folder name, without naming the git remote.

## Checked and correct

These statements were checked against the code and match.

**Tailscale Funnel and delete**

- The enable route requires `confirmed: true` and returns 400 without it, runs only for an active server in an active session, and returns 404 for an unknown session or server (`core/src/state.rs:2209-2237`, `core/src/api.rs:1136-1194`). The `confirmed` boolean is the contract and a direct local caller bypasses the dialog, as `SPEC.md:567` and `api.md:388` say.
- The command is exactly `tailscale funnel --bg <port>`, and the disable command is `tailscale funnel --https=443 localhost:<port> off` (`core/src/tailscale.rs:144-171`).
- One server is public at a time; enabling another turns the previous one off first (`core/src/state.rs:2238-2250`), serialized by the demo lock.
- Stop, close, pause, delete and app startup all turn Funnel off; close and pause do it before the agent wait or the Git work (`core/src/api.rs:2820-2833, 3733-3756`; `core/src/recovery.rs:26-81`).
- A refused turn-off keeps `funnelAddress` on the record and clears it only after the client accepts a command (`core/src/state.rs:2339-2373`; `core/src/sessions.rs:1925-1932`).
- Delete turns every public address off before the worktree, the files or the record; a refusal returns 422 with the reason, removes nothing and keeps the closed session and its address, so Delete can be retried (`core/src/api.rs:3859-3941`, `core/src/state.rs:2624-2643`; test `core/tests/api.rs:11718-11840`). This matches `SPEC.md:173, 572`, `api.md:390`, `docs/sessions.md:164` and `docs/session-tools.md:83`.
- The confirmation is modal and the app frame is inert; the dialog is portaled outside the panel (`app/src/components/DemoFunnelDialog.tsx:43-94`, `app/src/App.tsx:455-468`; e2e `e2e/24-demo-servers.spec.ts:198-220`).
- The files panel shows the public address only while the server is active, with one Stop public action (`app/src/components/SessionSidePanel.tsx:804-864`), as `SPEC.md:568` and `docs/session-tools.md:77` say.

**Phone UI package**

- Package paths are computer-scoped (`/ui/<computerId>/<buildPath>/`); the protocol handler resolves the build under the named computer, refuses another computer's build and requires an entry page (`phone-shell/src-tauri/src/ui.rs:458-568, 525-539`). Opening a computer again makes its stored build the served one (`:243-273`).
- The stored-build check and the install share one gate, so one update at a time can touch the build folders (`phone-shell/src-tauri/src/ui.rs:315-331`).
- The current, previous and on-screen builds are kept; an open page survives one and two updates; a build folder without an entry page is replaced (`:339-365, 773-813`; tests `:1316-1440`).
- `current.json` is written atomically, and a failed download, hash check or unpack leaves the previous build and the shell screens (`:386-456, 753-767`).
- The version gates and the update-needed states match `phone-shell.md:5.4` (`phone-shell/src-tauri/src/ui.rs:70-89`).

**Accessibility and UI behavior**

- The status badge shows "Active, Working", "Active, Paused", "Active, Waiting for you" or "Closed", and each state has a marker shape (`app/src/components/StatusBadge.tsx:9-24`, `app/src/state/sessionStatus.ts:15-91`; tests `app/src/components/StatusBadge.test.tsx:18-59`; e2e `e2e/09-switching.spec.ts:96-113`). This matches `SPEC.md:189` and `docs/sessions.md:85`.
- The session menu holds Reopen (closed), Rename, Move to project, Switch AI config (active agent), Copy folder path (no branch) and Close or Delete (`app/src/components/SessionActionsMenu.tsx:60-107`). A session without a branch has no branch chip (`app/src/components/SessionHeader.tsx:70-82`), so Copy folder path stays in its menu. This matches `docs/sessions.md:168-176` and `SPEC.md:153`.
- Move to project is the same action for drag, the menu and the M key; after a successful move, focus goes to the moved session's row or to the session list (`app/src/App.tsx:1645-1661`, `app/src/components/Sidebar.tsx:433-439`, `app/src/state/sessionRowFocus.ts:5-19`). This matches `SPEC.md:154, 187` and `docs/sessions.md:40-48`.
- The dock tab list is `tablist`/`tab`/`tabpanel` with Arrow/Home/End and `aria-controls` links (`app/src/components/SessionSidePanel.tsx:507-524, 621-675`). The browser tab strip moves with arrows only when the event target is a tab, and its tabs link to the tabpanel (`app/src/components/BrowserPanel.tsx:169-203, 404-467, 524-531`; test `BrowserPanel.test.tsx:253-254`). This matches `SPEC.md:190`.
- Focus rings draw inside the scrolling strips and the allowed-config list (`app/src/styles/app.css:48-53`; phone rows at `app/src/styles/phone.css:245-250, 339-344`), matching `SPEC.md:191`.
- Terminal output has the session name as its accessible name and polite live output (`app/src/components/TerminalPane.tsx:150-161`), matching `SPEC.md:192`.
- The phone sheets are modal, portaled outside the inert page, trap Tab, close on Escape and return focus to their trigger (`app/src/phone/PhoneApp.tsx:112-208`; e2e `e2e/36-phone-keyboard.spec.ts`). The start-session button uses `aria-disabled` and stays operable so a click shows the missing field (`app/src/phone/PhoneApp.tsx:1721-1745, 1866-1877`), matching `docs/phone.md:57`.
- Heading order has no skipped level: the app name and session name are `h1`, the sidebar and cards are `h2` (`app/src/App.tsx:1302`, `app/src/components/SessionHeader.tsx:45`, `app/src/components/Sidebar.tsx:578`, `app/src/components/PermissionCard.tsx:54`), matching `SPEC.md:184`.
- The start-session focus, the move focus and the panel-close focus are all implemented (`app/src/App.tsx:758-786`, `app/src/components/SessionSidePanel.tsx:156-169`), matching `SPEC.md:186-188`.

## Not checked

- A live Tailscale Funnel run against a tailnet. The e2e suite uses a fake client (`playwright.config.ts:22-45`), as `AGENTS.md:34` says.
- A packaged, signed macOS build; a phone device or emulator.
- Spelling and translation quality of the 12 catalogs.
