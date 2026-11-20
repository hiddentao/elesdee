# elesdee gap audit

Audit target: branch `hiddentao/round-2-mockups`, feature commit `a33b77a` (20 November 2026).
The branch advanced during the audit from `a458ac2` to `a33b77a`, then to `74c105c` (tests only);
the feature set is unchanged.
Sources of truth: `.dev/SPEC.md` and `.dev/phone-shell.md`. Code checked against both.

Scope notes:

- Per request, this audit excludes work in flight: the `link` crate internals, launch presets and
  model/config overrides, and the phone's own Languages setting.
- Commit `a458ac2` is the phone Languages sheet. It is excluded from the gap list.
- The feature commit `a33b77a` is core Git and file hardening. It compiles. It adds remote-ref
  checks and Git output draining, but not fetch/pull or the uncommitted-copy choice. File-path
  tests landed in `74c105c`; they are not part of this audit. This report judges the committed
  feature set.

## Update after the first fixes

The list below is a snapshot at `a33b77a`. These items have since closed and are no
longer gaps: crash recovery, the handoff note, the skill pack, Wait for session and
Share file, real usage sources for Claude Code, Codex and DeepSeek, launch fetch and
pull with the uncommitted-changes choice, launch presets, the phone Languages sheet,
the phone shell skeleton, the phone app connection step in progress, the manual
switch from the usage pill with the context-left value, and the allowed-provider
list with the spec's auto-switch order. Register demo server stays
open until demo servers exist.

## Verdict

The Mac session core is strong. Four clients, branches and worktrees, session lifecycle, the
sidebar, tabs, permissions, questions, session requests, the file and change panel, the CodeMirror
editor, project defaults, first prompt, source branch, environment overrides, account switching,
pause/resume, twelve languages and the quit flow are present.

The branch is not a Version 1 release. The largest remaining gaps are:

1. The provider usage pipeline is not connected to any official source, so the usage panel is
   empty and auto-switch never fires on real usage.
2. The handoff note, the six remaining skills and three session tools are missing, so the agent
   coordination layer is incomplete.
3. Crash recovery restores sessions as paused; it does not resume them, and no agent session ID is
   saved.
4. The phone shell, pairing and remote connection are not built; only the `link` crate and a web
   phone layout exist.
5. The built-in browser and demo servers are not built.
6. Launch does not fetch or pull, and it does not offer the uncommitted-changes choice.

## Ranked gaps

P0 means the first milestone or the core promise is incomplete. P1 means a Version 1 headline
feature is absent. P2 means completion, operations or polish.

| Rank | Gap | Spec section | Files that would change | Size |
| --- | --- | --- | --- | --- |
| P0 | Crash recovery is not automatic: sessions restore paused and no agent session ID is saved | Crash recovery | `core/src/sessions.rs`, `state.rs`, `persist.rs`, `api.rs`, `claude.rs`, `codex.rs`, `opencode.rs`, `antigravity.rs` | L — 1–2 weeks |
| P0 | Handoff note is absent at close, client switch and out-of-usage pause; no Handoff skill | Handoff note; Closed sessions; Manual switch; Auto-switch | `core/src/git.rs`, `api.rs`, `state.rs`, `persist.rs`, `mcp.rs` | L — 1–2 weeks |
| P0 | Six of seven skills are absent, and Codex gets no skill | Skills given to every session; How sessions get them | `core/src/mcp.rs`, `api.rs`, new skill assets | M — 1 week |
| P0 | Three session tools are absent: Wait for session, Register demo server, Share file | Session tools | `core/src/mcp.rs`, `state.rs`, `api.rs`, `app/src` view for shared files | M — 3–5 days |
| P0 | No official usage source feeds the panel or auto-switch | Where usage numbers come from; Usage panel | `core/src/claude.rs`, `codex.rs`, new usage module, `providers.rs`, `api.rs`, `state.rs`, `app/src` | L — 2–3 weeks |
| P0 | No allowed-provider list per session; auto-switch ignores the spec's order and never crosses clients | Allowed providers per session; Auto-switch rules | `core/src/state.rs`, `api.rs`, `providers.rs`, `persist.rs`, `app/src/components/NewSessionDialog.tsx` | M — 3–5 days |
| P0 | Launch does not fetch the remote or pull a tracking branch; no uncommitted-changes choice | Auto-sync at launch; Sources | `core/src/git.rs`, `api.rs`, `app/src/components/NewSessionDialog.tsx`, `app/src/api/types.ts` | M — 3–5 days |
| P0 | Manual switch is a menu action; the usage pill does not switch, and there is no context-left data | Manual switch; Usage panel | `app/src/components/UsagePill.tsx`, `SessionHeader.tsx`, `SwitchAccountDialog.tsx`, `core/src/api.rs`, `sessions.rs` | M — 3–5 days |
| P0 | Subscription sign-in is metadata only: no agent login, browser return, credential check or one-time code | Subscription sign-in | new `core/src/auth.rs`, `api.rs`, `clients.rs`, `secrets.rs`, `app/src/components/AddAccountDialog.tsx`, `client.ts` | L — 2–3 weeks |
| P1 | Built-in browser is absent, with the Browser tab, per-session context and Playwright tools | Browser tools; Session side panel | new `core/src/browser/`, `mcp.rs`, `state.rs`, `api.rs`, `app/src/components/SessionSidePanel.tsx`, new browser view, packaging | XL — 4–8 weeks |
| P1 | Demo servers are absent, including the register tool, forwarding and Tailscale | Demo servers; Session tools; Session side panel | new `core/src/demo.rs`, `api.rs`, `mcp.rs`, `state.rs`, phone shell forward, `app/src` | XL — 3–5 weeks |
| P1 | Phone shell, pairing, core endpoint and remote connection are absent; only the link crate and web layout exist | Mobile companion; Connection technology; `phone-shell.md` steps 2–9 | `core/src/link/`, `api.rs`, `state.rs`, `persist.rs`, `link/src/client.rs`, new `phone-shell/`, `app/src/api/transport.ts`, `app/vite.phone.config.ts`, `scripts/package-phone-ui.mjs`, CI and release | XL — 8–12 weeks |
| P1 | The phone message view is absent: the phone shows a read-only terminal, and Codex/OpenCode have no history or item feeds | How a session looks; Where the message view gets its data | `core/src/claude.rs`, `api.rs`, `codex.rs`, `opencode.rs`, `state.rs`, `persist.rs`, `app/src/phone/PhoneApp.tsx` | XL — 4–6 weeks |
| P1 | Phone mobile actions and the keyboard rail are absent: input, upload, files/images/diffs, start/close/switch/usage, notifications, key rail | From the mobile app; Keyboard input; Notifications on the phone | `app/src/phone/PhoneApp.tsx`, new phone components, `core/src/api.rs` and link dispatch, `phone-shell/` | L — 3–5 weeks |
| P1 | Account edit and remove are absent; Settings has no provider configuration or auto-switch setting | Custom providers; Settings / Providers | `core/src/api.rs`, `state.rs`, `secrets.rs`, `app/src/components/SettingsDialog.tsx`, new account edit dialog, `client.ts` | M — 3–5 days |
| P1 | Android app, APK, self-update and Play build are absent | Android app; Releases and updates | new `phone-shell/` Android project, CI and release workflows | XL — 3–5 weeks after the shell |
| P2 | Settings has no General page: start at login and keep awake are absent | Desktop app architecture; Settings | `app/src/components/SettingsDialog.tsx`, new core settings API, `src-tauri` plugins | M — 1 week |
| P2 | Settings has no Devices page: paired phones and Remove are absent | Settings; After pairing | `app/src/components/SettingsDialog.tsx`, core device store and routes, phone shell | M — 3–5 days |
| P2 | No local log file and no Export diagnostics action | Privacy; Settings | `core/src/main.rs`, new diagnostics module, `api.rs`, `SettingsDialog.tsx`, `src-tauri` | M — 3–5 days |
| P2 | No desktop updater: no check, download, signature check or install flow | Update checks; Update while sessions run | `src-tauri/Cargo.toml`, `tauri.conf.json`, `src-tauri/src/main.rs`, update UI, release workflow | L — 2–3 weeks |
| — | Built: a missing agent is installed from the AI config form after confirmation, with progress and a version check | Agent support | `core/src/clients.rs`, `core/src/api.rs`, `app/src/components/AiConfigDialog.tsx` | Done |
| — | Built: clients are found on PATH, in user npm/Node folders, in Homebrew on both architectures, in the app bundle and in the app's own data folder | Agent support | `core/src/clients.rs`, `core/src/state.rs` | Done |
| P2 | A session cannot be dragged from one project to another | Projects | `app/src/components/Sidebar.tsx`, `app/src/App.tsx`, `core/src/api.rs`, `app/src/state/tabs.ts` | M — 3–5 days |
| P2 | Antigravity gets no MCP server and no skills; the spec's introduction says every agent session gets them | Built-in tools and skills | `core/src/api.rs`, `mcp.rs`, `antigravity.rs` | S — 2–4 days after checking `agy` MCP support |
| P2 | GitHub is not connected; there is no repository search or recent-repository picker | Sources; Git access | new `core/src/github.rs`, `api.rs`, `secrets.rs`, `app/src/components/NewSessionDialog.tsx` | L — 2–3 weeks |
| P2 | A custom provider's model is not checked against the server's model list | Custom providers | `core/src/providers.rs`, `api.rs`, `app/src/components/AddAccountDialog.tsx` | S — 1–2 days |

## Detail

### P0 — first milestone and core promise

**1. Crash recovery.**
Spec: Crash recovery. The spec says every running session reopens and resumes, the agent's own
session ID is saved, and the user does nothing. The code restores a saved active session as
`Paused` with `"Paused. The app restarted. Resume to continue."` (`core/src/sessions.rs`).
`PersistedSession` saves the session, columns, rows, output, sequence and repo root, and no agent
session ID. Reopen and resume use `resume_arguments`: `--continue` for Claude, Antigravity and
OpenCode, and `resume --last` for Codex. That resumes the last conversation in the worktree, not a
saved ID. Terminal sessions also restore paused. Files: `core/src/sessions.rs`, `state.rs`,
`persist.rs`, `api.rs`, `claude.rs`, `codex.rs`, `opencode.rs`, `antigravity.rs`. Size: L.

**2. Handoff note.**
Spec: Handoff note; Closed sessions; Manual switch; Auto-switch. The close commit message is
`elesdee: close <name>` (`core/src/git.rs`). No handoff note is written, stored or copied into
the last commit. A client switch starts the new client without a handoff summary
(`core/src/api.rs`). A pause stops the process without commit, push or note
(`core/src/api.rs`). The Handoff skill is absent. Files: `core/src/git.rs`, `api.rs`,
`state.rs`, `persist.rs`, `mcp.rs`. Size: L.

**3. Skill pack.**
Spec: Skills given to every session. The code writes one skill, `elesdee`, as a plugin
(`core/src/mcp.rs`). Missing: Demo servers, Handoff, Browser checks, Ask and notify,
Coordinate sessions and Frontend design. The elesdee skill covers a subset of Ask and notify and
Coordinate sessions, but the separate skills and their guidance are absent. Claude Code gets the
plugin through `--plugin-dir`; OpenCode gets it through its config. Codex gets the MCP server but
no skill (`core/src/api.rs`). The spec lists Codex among the clients that read SKILL.md.
Files: `core/src/mcp.rs`, `api.rs`, new skill assets. Size: M.

**4. Missing session tools.**
Spec: Session tools. The MCP server exposes `ask_me`, `wait_for_answer`, `withdraw_question`,
`notify_me`, `report_done`, `list_sessions`, `read_session`, `send_message`, `check_usage` and
`start_session` (`core/src/mcp.rs`). Missing: Wait for session, Register demo server and
Share file. Register demo server depends on the demo-server work. Share file needs a shared-file
store and a UI. Wait for session needs a wait on another session's status. Files:
`core/src/mcp.rs`, `state.rs`, `api.rs`, and a shared-files view in `app/src`. Size: M.

**5. Official usage sources.**
Spec: Where usage numbers come from; Usage panel. The only way to set usage rows is
`POST /api/accounts/{id}/usage` (`core/src/api.rs`). The app never calls it; only the
end-to-end helper does (`e2e/helpers.ts`). There is no Claude status-line script, no Codex
`account/rateLimits/read` or `account/rateLimits/updated`, and no DeepSeek balance request. The
panel therefore shows sample accounts with no rows, and auto-switch never sees `NearLimit`. Files:
`core/src/claude.rs`, `codex.rs`, a new usage module or `providers.rs`, `api.rs`, `state.rs`, and
the usage components for "last read" text. Size: L.

**6. Allowed providers and auto-switch rules. Closed.**
Spec: Allowed providers per session; Auto-switch rules. `Session` has no allowed-provider list
(`core/src/state.rs`). Auto-switch targets only accounts with the same client as the
current one, filters out samples, and picks the highest remaining percent
(`core/src/api.rs`). It does not prefer subscriptions over pay-per-use accounts, and it
does not cross clients. Resume picks any same-client account with room
(`core/src/api.rs`). Files: `core/src/state.rs`, `api.rs`, `providers.rs`, `persist.rs`,
and the launch form. Size: M.

**7. Launch auto-sync and uncommitted changes.**
Spec: Auto-sync at launch; Sources. `create_session` accepts a source branch and passes it to
`git worktree add -b <branch> <path> <base>` (`core/src/git.rs`). It does not fetch a
remote or pull a tracking local branch. It does not ask to copy uncommitted changes; the new
worktree starts from the source ref. The branch combobox lists local and remote refs
(`core/src/git.rs`), but the remote ref may be stale, and there is no explicit local /
remote / plain-folder source choice. Files: `core/src/git.rs`, `api.rs`,
`app/src/components/NewSessionDialog.tsx`, `app/src/api/types.ts`. Size: M.

**8. Manual switch completeness. Closed for the switch and the context display.**
Spec: Manual switch; Usage panel. Clicking the provider pill now opens the chooser. It lists
every account the switch endpoint accepts with its remaining usage and marks the automatic
switch target. The pill tooltip shows the session's context left. A different-client switch
sends the handoff note and the end of the log. The allowed-provider list and the
subscription-first cross-client switch order are now built (gap 6). Remaining: a context
figure for Codex, OpenCode 2 and Antigravity, whose clients expose no official field yet.

**9. Subscription sign-in — closed.**
The core `auth` module starts `claude auth login` or `codex login` under a terminal, parses the
sign-in address and a device code, forwards a pasted one-time code, and asks the client to confirm
the credential (`claude auth status` and `codex login status`) before it creates or reconnects the
provider. The provider records `connected`; no token is read, stored or logged. The provider form
and Providers page show the waiting, code, failure and connected states. A fake client covers
success, refusal, timeout, abandonment, the one-time code, a key-only login and the usage feed in
the core tests; the UI covers the form states and a stubbed bridge covers the flow end to end.
A real Anthropic or OpenAI account is still needed to check the vendor pages themselves, a real
browser callback, a real device-code grant and the token refresh over time. Files: `core/src/auth.rs`,
`api.rs`, `state.rs`, `providers.rs`, `app/src/components/ProviderDialog.tsx`, `SettingsDialog.tsx`,
`client.ts`, catalogs. Size: L.

### P1 — Version 1 headline features

**10. Built-in browser.**
Spec: Browser tools; Session side panel. No browser code exists. The side panel has only Files and
Changes tabs (`app/src/components/SessionSidePanel.tsx`). The CI Playwright Chromium is for
tests only. Missing: the Chromium download, per-session contexts and encrypted logins, the
Playwright MCP wrapper and its tools, the live view, the Browser tab and phone screenshots. The
spec itself marks the Browser tab as later, but the browser is a Version 1 item. Files: new
`core/src/browser/`, `mcp.rs`, `state.rs`, `api.rs`, `app/src/components/SessionSidePanel.tsx`,
new browser view, packaging. Size: XL.

**11. Demo servers.**
Spec: Demo servers; Session tools; Closed sessions. No demo-server code exists. Missing: the
register tool, a port registry, the phone forwarding path, the optional Tailscale check and serve,
the demo-server skill, the UI list, and stopping servers on close or pause. Files: new
`core/src/demo.rs`, `api.rs`, `mcp.rs`, `state.rs`, phone shell forward, `app/src`, Tailscale
integration. Size: XL.

**12. Phone shell, pairing and connection.**
Spec: Mobile companion; Connection technology; `.dev/phone-shell.md` steps 2–9. The `link` crate
(step 1) exists and is in flight. There is no `phone-shell/`, no core link endpoint, no pairing
state, QR or device store, no `/api/pairing` or `/api/devices` routes, no UI transport adapter, no
phone UI package build, no bridge, no reconnect and no Android project. The web layout at
`phone.html` is a preview, not the shell. Files: `core/src/link/` (new), `core/src/api.rs`,
`state.rs`, `persist.rs`, `link/src/client.rs` and server helpers, new `phone-shell/`,
`app/src/api/transport.ts`, `app/vite.phone.config.ts`, `scripts/package-phone-ui.mjs`,
`.github/workflows`, release. Size: XL.

**13. Phone message view and agent event feeds.**
Spec: How a session looks; Where the message view gets its data. The phone shows a read-only
terminal (`app/src/phone/PhoneApp.tsx`), not the message view. The core stores no prompt,
reply or tool records. Claude hooks update preview and permissions only. Codex starts the
app-server and reads approval requests only (`core/src/codex.rs`); it does not read
`thread/turns/list`, `thread/items/list` or live `item/*`. OpenCode polls its permission list only
(`core/src/opencode.rs`). Files: `core/src/claude.rs`, `api.rs`, `codex.rs`, `opencode.rs`,
`state.rs`, `persist.rs`, `app/src/phone/PhoneApp.tsx`. Size: XL.

**14. Phone mobile actions and keyboard rail.**
Spec: From the mobile app; Keyboard input; Notifications on the phone. The phone layout can show
sessions, permission cards, questions and session requests. It has no composer, upload, files,
images, diffs, start or close, account switch, usage panel, keyboard rail, saved keys, or
notifications. `Session.shortcuts` is returned by the server but unused on the phone. Files:
`app/src/phone/PhoneApp.tsx`, new phone components, `core/src/api.rs` and link dispatch, `phone-shell/`.
Size: L.

**15. Account edit and remove.**
Spec: Custom providers; Settings / Providers. The API has account list, create and usage update
only. There is no `PATCH` or `DELETE` account route, no edit dialog and no remove action. Settings
lists accounts but has no configure action, no auto-switch setting and no account detail. Files:
`core/src/api.rs`, `state.rs`, `secrets.rs`, `app/src/components/SettingsDialog.tsx`, new edit
dialog, `client.ts`. Size: M.

**16. Android app and release.**
Spec: Android app; Releases and updates. No Android project, APK, self-updater, Play build or
Android CI job exists. Files: `phone-shell/` Android project, `.github/workflows`, signing. Size:
XL, after the shell.

### P2 — operations, settings and polish

**17. Settings General.** Spec: Desktop app architecture; Settings. Settings has Providers,
Languages and About only (`app/src/components/SettingsDialog.tsx`). Start at login and keep
awake are absent, and `src-tauri` has no autostart or power plugin. Files: `SettingsDialog.tsx`,
new core settings API, `src-tauri/Cargo.toml`, `src-tauri/src/main.rs`. Size: M.

**18. Settings Devices.** Spec: Settings; After pairing. No paired-device list, remove action or
pairing UI. Depends on the phone pairing work. Files: `SettingsDialog.tsx`, core device store and
routes, phone shell. Size: M.

**19. Diagnostics and logs.** Spec: Privacy; Settings. The core logs JSON to stderr; there is no
log file and no Export diagnostics action. Files: `core/src/main.rs`, new diagnostics module,
`api.rs`, `SettingsDialog.tsx`, `src-tauri`. Size: M.

**20. Desktop updater. Built.** The shell checks this repository's latest release
through `tauri-plugin-updater` at launch and every few hours, downloads the
package in the background, verifies its signature and offers Install on the
About page. A running session asks before the restart; choosing Later installs
the update when every session is closed. A build without the update public key
reports that updates are unavailable. The release workflow publishes the signed
`.app.tar.gz` and `latest.json`. Real checks, downloads and installs need a
packaged, signed app and a release signed with the update key.

**21. Notifications. Built.** The desktop shell uses the Tauri notification plugin. A session that
waits for input, a permission answer or a new-session approval, an agent that reports done, and the
Notify me tool raise a desktop notification. A notification click opens that session. See
"What triggers a notification" in the spec.

**22. Missing agent install. Built.** Spec: Agent support. The AI config form lists the clients
from the health check and shows one Install action for each missing client. After a confirmation,
the core runs the vendor's install script into a private staging folder under the data directory,
checks the installed copy's version, rewrites absolute links and replaces any older app copy. A
failed install removes the staging copy, keeps the older copy and returns the error. The install
route reports its stage and progress. Files: `core/src/clients.rs`, `core/src/api.rs`,
`app/src/components/AiConfigDialog.tsx`, `app/src/api/client.ts`.

**23. Packaged agent discovery. Built.** Spec: Agent support. `find_client` checks each `PATH`
folder, the user npm, Node and `~/.local/bin` folders, the Antigravity folders, Homebrew on Apple
Silicon and Intel, the app bundle and the app's own data copy. Relative `PATH` folders are ignored
and no login shell runs. The health check returns the path and version. Files:
`core/src/clients.rs`, `core/src/state.rs`.

**24. Session drag between projects.** Spec: Projects. There are no drag handlers or drop targets
in the sidebar or session list. Moving a session needs a route that changes its project and a tab
and filter update. Files: `app/src/components/Sidebar.tsx`, `app/src/App.tsx`, `core/src/api.rs`,
`app/src/state/tabs.ts`. Size: M.

**25. Antigravity tools and skills.** Spec: Built-in tools and skills. `configure_session_tools`
has an empty Antigravity branch (`core/src/api.rs`). The spec's introduction says every agent
session gets the same tools and skills; the implementation gives them to Claude, Codex and
OpenCode only. Check whether `agy` supports MCP before adding the server. Files: `core/src/api.rs`,
`mcp.rs`, `antigravity.rs`. Size: S.

**26. GitHub connection.** Spec: Sources; Git access. There is no GitHub device-code connection,
token storage or repository search. The launch form uses the project folder and a branch combobox.
Files: new `core/src/github.rs`, `api.rs`, `secrets.rs`, `app/src/components/NewSessionDialog.tsx`,
`AddProjectDialog.tsx`. Size: L.

**27. Custom model list check.** Spec: Custom providers. The model is free text. The app does not
ask the server for its model list and accept the name when the server cannot answer. Files:
`core/src/providers.rs`, `api.rs`, `app/src/components/AddAccountDialog.tsx`. Size: S.

## Already built and checked

- Four clients: Claude Code, Codex, OpenCode 2 and Antigravity. Detection and versions are in the
  health response. Antigravity runs with a per-session home, private settings and the key in the
  child environment.
- Accounts: add for Anthropic, OpenAI, Google, Kimi, DeepSeek, Z.ai, OpenRouter, Ollama, LM Studio
  and custom OpenAI- or Anthropic-compatible endpoints. Subscription, API-key and local-server
  metadata. Keys in the system keychain. Sample accounts have no usage. Secrets are redacted from
  session output.
- Sessions: agent and terminal, branch and worktree, close/reopen/delete, rename, previews,
  persistence, the 64-session cap, and the first-launch flow with no projects.
- Sidebar: grouped and flat, filters, blots, questions and request chips, parent "from" marker.
  Per-project tabs. Session menu, including account switch.
- Permissions: Claude mutating-only matcher; Codex app-server approvals; OpenCode permission
  polling; Antigravity keeps its own prompts. Phone layout cards.
- Questions and session requests: ask, wait, withdraw, answer; start-session requests with
  "allow further"; parent marker.
- Side panel: Files and Changes, CodeMirror editor with explicit save, added/removed counts and
  diffs, plain-folder and binary handling.
- Project defaults: default account and source branch, prefilled in the launch form. First prompt,
  source branch, environment overrides and the Tauri folder picker.
- Twelve language catalogs, the Settings Languages page and the phone language sheet.
- Quit dialog and Tauri close handling; pause/resume; auto-switch logic (not fed by real usage).
- MCP server and ten tools; the elesdee skill for Claude and OpenCode; Codex MCP wiring.
- Desktop notifications for a waiting session, a done report and Notify me, with a click that
  opens the session. The focused window skips a notice it already shows.
- E2E suite, CI, macOS release workflow with optional signing and notarization.

## Excluded by request

- The `link` crate internals (step 1 of `phone-shell.md`) and its tests.
- Editable launch presets and model/config overrides. Environment overrides are built.
- The phone's own Languages setting (commit `a458ac2`).
- The uncommitted core change in the worktree, which is not a spec feature and did not compile at
  audit time.

## Method

I read `.dev/SPEC.md`, `.dev/phone-shell.md`, `.dev/api.md`, `.dev/hooks.md` and `AGENTS.md`, then
checked the core, UI, Tauri shell, link crate, e2e suite and release workflows. I confirmed the
committed HEAD compiles. I did not change any file except this report.
