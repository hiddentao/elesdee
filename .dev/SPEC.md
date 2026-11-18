# elesdee — Spec

Last updated: 18 November 2026

## Purpose

Build an IDE that runs Claude Code and Codex sessions the way I want, with less manual work. It replaces onoRCA.dev, which does not give enough control and does not save enough time.

The app is public. Anyone can install it from this repository's GitHub releases.

**Version 1 (first public release)** includes:

- Agent sessions, each with its own branch and worktree, and terminal sessions
- Presets and config overrides
- Crash recovery
- The Android phone app, with QR pairing
- Provider switching: usage panel, one-button switch and auto-switch
- The built-in browser for agents
- The session tools and skills that let agents start and coordinate sessions, ask me questions and notify me
- The file viewer and code editor side panel
- Demo servers that open on my phone (see Demo servers)

Version 1 runs on macOS only. Windows and Linux follow soon after (see Windows and Linux).

**Build order**

1. UI mockups of the Mac window and the phone layout, in several rounds, before any code is written.
2. The iroh test build (see Connection technology).
3. First milestone: sessions on the Mac. Agent and terminal sessions, worktrees, presets, crash recovery, and the session tools and skills that agents need from the start.
4. The Android phone app.
5. Provider switching.
6. The built-in browser.
7. The file viewer and code editor.
8. Demo servers.

**Privacy**

- The app sends no crash reports and no usage data. Users report bugs on GitHub.
- The app keeps local logs. An "Export diagnostics" button saves one file with the logs and app versions, with secrets and session content removed, for the user to attach to a GitHub issue.

**Languages and formats**

The approach follows hezo.ai's languages and formats ([hezo docs](https://github.com/hezo-ai/hezo/blob/main/docs/concepts/languages-and-formats.md)).

- The interface comes in 12 languages: English, Deutsch (German), Français (French), Español (Spanish), Italiano (Italian), Português do Brasil (Brazilian Portuguese), Nederlands (Dutch), Polski (Polish), Svenska (Swedish), 简体中文 (Simplified Chinese), 日本語 (Japanese) and 한국어 (Korean).
- European Portuguese falls back to Brazilian Portuguese, and Traditional Chinese falls back to Simplified Chinese. There are no right-to-left languages.
- On first launch, the app uses the Mac's system language. A language button on the first screen lets me pick another one.
- Settings has a "Languages & formats" page with three separate choices. Their defaults follow the language, and each option shows a real example:
  - Language.
  - Date format: day first (29/07/2026), month first (07/29/2026), year first ISO (2026-07-29), or day, month name, year (29 Jul 2026). Month names follow the language.
  - Number format: 1,234.56, 1.234,56, or 1 234,56.
- The phone: the UI package holds all languages. The phone uses its own system language, and has its own "Languages & formats" setting, so the phone and the Mac can differ.
- The phone shell's own screens (pairing, the list of Macs, "update needed") are translated inside the shell, because they show before any UI package is downloaded.
- What is not translated: agent output, prompts, session logs and other session content stay in the language they were written in. The skills and starting instructions that elesdee gives agents stay in English. To make an agent work in another language, I say so in a preset or in my prompt. The documentation is in English only.
- Interface text is kept in one catalog per language. Each pull request that adds interface text adds it in all 12 languages, translated by an agent. CI fails if any language is missing a text.

**Moving to a new Mac**

- There is no export, import or sync. I set up a new Mac from scratch.

This spec is a first draft. More requirements will follow. Detailed research notes, with quotes and sources, are in `.dev/research/`.

## Desktop app architecture

The desktop app is a Tauri app. Version 1 is for macOS; Windows and Linux follow soon after, so the core service must not depend on macOS-only features. Its core is a separate Rust process, the core service. The app starts and stops it. It is not installed as a macOS system service. The window only shows what the core service does.

| Part | Job |
| --- | --- |
| Core service (Rust) | Runs agent sessions in terminals, manages Git branches and worktrees, tracks usage, saves session state, holds the iroh connection to the phone |
| Desktop window (Tauri) | Shows sessions, usage and projects. Sends my input to the core service. |
| Mobile app (Tauri shell) | A small shell that runs the UI package it downloads from the desktop app, and talks to the core service over iroh |

- If the window crashes or I close it, sessions keep running and the phone stays connected. When the window opens again, it reconnects.
- Start at login: a setting, "Start elesdee when the system starts", is off by default. When it is on, the core service starts when I log in and resumes the sessions; the window opens only when I click the app. When it is off, elesdee starts only when I open it.
- Sleep: a setting, "Keep the Mac awake while agents work", is off by default. When it is on, the Mac does not go to sleep on its own while any agent session is running. When it is off, sessions pause while the Mac sleeps, and the phone cannot connect.
- When I quit the app while sessions are running, the app asks me first. If I confirm, the core service saves every session and stops. On the next launch, the sessions resume. Crash recovery (below) covers a crash of the core service or a restart of the Mac.
- All parts live in this one repository: the core service and the shared connection library (Rust), the Mac app (Tauri), the phone shell (Tauri mobile) and the shared UI (React). One change can update them together, and one release workflow builds them.
- Every pull request runs lint and format checks, unit tests, the Mac and Android builds, a check that every interface text exists in all 12 languages, and end-to-end tests that start the app and drive real sessions with a test agent.
- Why Tauri and not Electron: iroh is written in Rust, so the service uses it directly. Tauri also uses less memory, which matters for an app that runs all day. On macOS, Tauri's built-in web view works well.

## Window layout and sessions

Sessions are the main view. A file viewer and code editor is an extra panel that I open only when I need it. The layout will be settled through several rounds of UI mockups, before any code is written.

**How a session looks**

- On the Mac, an agent session shows the agent's real terminal.
- On the phone, it shows a simpler message view, built from structured events of the conversation. The real terminal is one tap away.
- The message view shows my prompts and the agent's replies in full. Tool calls (commands, file edits) appear as one-line rows that I can tap to open.

**Where the message view gets its data**

Both agents write their conversation to files on disk, but both say the file format is not a public interface. Claude Code: "The entry format is internal to Claude Code and changes between versions, so scripts that parse these files directly can break on any release" ([Claude Code docs](https://code.claude.com/docs/en/sessions)). Codex: "the transcript format isn't a stable interface for hooks and may change over time" ([Codex docs](https://learn.chatgpt.com/docs/hooks)). So the app does not parse these files. It uses each agent's official event interface, which runs next to the real terminal.

- Claude Code: hooks, added at launch through `--settings`, like the other launch overrides.
  - HTTP hooks "send the event's JSON input as an HTTP POST request to a URL" ([Claude Code docs](https://code.claude.com/docs/en/hooks)). The URL is the core service on localhost.
  - Events for the message view: `UserPromptSubmit` (my prompt), `MessageDisplay` (assistant text, which "runs while an assistant message streams to the screen"), `PreToolUse` and `PostToolUse` (tool calls and results), `Stop` (the final reply) and `Notification` (permission prompts) ([Claude Code docs](https://code.claude.com/docs/en/hooks)).
  - `MessageDisplay` holds the terminal's rendering until the hook returns, so the core service must answer at once.
- Codex: the app-server protocol, shared with the terminal UI.
  - The core service starts `codex app-server` on a local socket and launches the terminal UI with `codex --remote unix://PATH`, so the terminal UI is a client of that server ([Codex docs](https://learn.chatgpt.com/docs/app-server)).
  - The core service connects its own client to the same server. It reads history with `thread/turns/list` and `thread/items/list`, and follows live `item/*` events ([Codex docs](https://learn.chatgpt.com/docs/app-server)).
  - Risks: the `codex app-server` command is labelled experimental. The docs do not say outright that a second client can follow a thread that the terminal UI is running. The test build must check this.

**Session types**

- Agent session: runs Claude Code or Codex, with a preset, provider, branch and worktree.
- Terminal session: a plain terminal, with no agent. It sits under a project in the sidebar and opens in a folder I choose, often an agent session's worktree. It has no branch or worktree of its own.

**Session names and limits**

- The app names each agent session from its first prompt, with the same short name used in the branch name. I can rename a session later; its branch keeps its name.
- A terminal session is named after its folder. I can rename it.
- There is no limit on how many sessions run at the same time, and no warning. I manage it myself.

**Sessions sidebar**

- Shows all sessions in a vertical list, in one of two views:
  - Grouped: sessions sit under their project. I can collapse a project. A collapsed project shows a pulsing dot when a session under it is running.
  - Flat: no project groups. Each session shows its project name in small text under the session name.
- A filter shows only running sessions. In the grouped view, the filter expands every project that has a running session.

**Session menu**

- Right-click a session in the sidebar, or its tab, to open a menu of actions.
- One action is Close. Close stops the session. The app asks me to confirm first.
- Close is the only way to stop a session for good. The app can also pause a session that is out of usage (see Auto-switch).

**Closed sessions**

- A closed session disappears from the sidebar.
- A filter at the top of the sidebar, "Show closed sessions", brings closed sessions back into the list. They look different from open sessions, for example greyed out.
- I can reopen a closed session. The app opens it in a new tab, loads its full log, and restarts the agent with the provider it last used.
- If the agent can resume its own saved conversation, it does. If not (for example after a switch to a different agent), it gets the handoff note and the end of the log, because a long log may not fit in its context window.
- Close keeps the session's branch but deletes its worktree folder, to save disk space. The steps:
  1. The agent commits its work, writes the handoff note and pushes the session branch to the remote.
  2. If anything is still uncommitted or unpushed, the app commits and pushes it, so no work is lost.
  3. If the push fails, or the branch has no remote, the app tells me before it deletes the worktree folder. The branch stays on my Mac.
  4. The app deletes the worktree folder.
- A session on a plain folder (no Git) has no branch or worktree. Close only stops it; the folder stays as it is.
- The app does not open a pull request. I do that myself.
- Reopen re-creates the worktree from the session branch.
- At Close and at pause, the app stops the session's demo servers and closes its browser tabs. Browser logins are kept.
- Closed sessions stay until I delete them. For a closed session, the session menu has Delete, which removes its log from the app. The branch stays in Git.

**Tabs**

- Click a session in the sidebar to open it as a tab.
- A tab cannot be closed, only minimized. Minimize takes the tab out of the tab bar. The session keeps running and stays in the sidebar.

**File viewer and code editor (side panel)**

- Closed by default. I open it when I want to look at or edit files.
- Shows the file tree of the current session's worktree or folder.
- Code editor with full syntax highlighting. I can edit and save files.
- Not a main feature: it supports quick edits, not full IDE work.
- Mac only for now. The phone does not have the file viewer and editor yet.
- The editor is CodeMirror: light, with full syntax highlighting, and it works on touch screens, so it can come to the phone later.

## Agent support and presets

Version 1 supports two coding agents: Claude Code and Codex. Other agents can come later.

- elesdee uses the Claude Code and Codex that I installed, and shows their versions.
- If one is missing, elesdee installs its own copy.

**Permission prompts**

- When Claude Code or Codex asks for permission before an action (for example running a command or editing a file), the prompt appears in the session view on the Mac and on the phone.
- I can approve or deny from either device. The session shows a "waiting for you" badge in the sidebar until I answer.

**Config overrides**

- Each launch can override the agent's environment variables and config settings.
- Overrides apply only to that launch. They do not change the global config on the machine.
- Overrides are easy to see and edit in the IDE, not hidden in files.

**Built-in presets (version 1)**

| Preset | Agent | Model provider | What the preset sets |
| --- | --- | --- | --- |
| Anthropic Claude | Claude Code | Anthropic | Claude subscription login |
| DeepSeek | Claude Code | DeepSeek | DeepSeek endpoint, API key, model names |
| Codex | Codex | OpenAI | ChatGPT/OpenAI login |

- I can edit presets and add new ones.
- Presets and config overrides are stored in elesdee's own settings on the Mac. Nothing is written into my repos.

## Built-in tools and skills for sessions

Every agent session gets the same app tools and skills, whatever the agent or provider. They come from elesdee itself, not from a provider's add-on such as a browser extension.

**How sessions get them**

- The core service runs one MCP server, "elesdee". Claude Code and Codex both support MCP servers ([Codex docs](https://developers.openai.com/codex/mcp)).
- The server listens on localhost and has no access check: any program on the Mac can call it. Starting a session still needs my approval, so a local program cannot start agents on its own.
- Each session gets its own address on the server, so the server knows which session made each call.
- At launch, the app adds this MCP server and the skills to the agent through the launch overrides. It does not change global config or write into the repo.
- Skills use the SKILL.md format, which both Claude Code and Codex read ([Codex docs](https://developers.openai.com/codex/skills.md)).
- Agents read only a skill's name and description at start, and load the rest when they decide they need it. So a short note in each agent's starting instructions also tells it that it runs inside elesdee and points it to the elesdee skill.

**Browser tools**

- The core service runs its own browser (Chromium), separate from my personal browser. No browser extension is needed.
- Chromium is not bundled in the app. The app downloads it the first time a session needs a browser. This keeps the app download and its updates small.
- Each session gets its own browser context: its own tabs, cookies and storage. The session keeps its logins when I quit and relaunch the app, after a crash or a restart of the Mac, and when I close and reopen the session. Login data is stored encrypted on my Mac.
- Tools: open a page, click, type, read the page, take screenshots, read console and network logs.
- On the Mac, I can watch a session's browser live and click or type in it, for example to log in to a site for the agent.
- The phone has no live view of the browser. It gets the screenshots that agents share.
- The elesdee server wraps Microsoft's Playwright MCP server, which already has 70+ browser tools ([Playwright](https://playwright.dev/mcp/introduction)). It runs Playwright MCP for each session and passes its tools through, so agents see one server. elesdee adds the per-session logins and the live view.

**Session tools**

| Tool | What it does |
| --- | --- |
| List sessions | Lists sessions with their project, status and branch |
| Start session | Starts a new session with a preset, project, source branch and first prompt. I must approve each new session first, on the desktop or the phone. The approval has an option, "Allow further sessions from this session", so a coordinating agent can start helpers without more questions. The sidebar marks which session started it. |
| Send message | Sends input to another session |
| Read session | Reads another session's recent output and status: running, waiting for input, or finished |
| Report done | Tells the app that this session's task is done. Only then does the session count as finished; an idle agent counts as waiting for input. |
| Wait for session | Waits until another session finishes or needs input |
| Register demo server | Tells the app that a demo server runs on a port. Returns an address I can open remotely (see Demo servers). |
| Notify me | Sends me a notification on the desktop, and on the phone while the phone app is open |
| Ask me | Asks me a question and waits for my answer, on the desktop or the phone |
| Share file | Adds a file or screenshot to the session's view, so I can see it on any device |
| Check usage | Shows the session's remaining context and the provider's remaining usage |

- An agent cannot close a session. Only I can, through the session menu on the Mac or the phone.

**Skills given to every session**

| Skill | What it teaches |
| --- | --- |
| elesdee | What elesdee is. The session has its own branch and worktree. How to use the session tools to start and coordinate other sessions. |
| Demo servers | Start a demo server, register its port, and give me the address that works on my phone |
| Handoff | The standard handoff note, and when to commit, write it and push: switch to a different agent, out of usage, Close |
| Browser checks | Check UI changes in the built-in browser, take screenshots and share them with me |
| Ask and notify | Ask me through the app when blocked, instead of waiting. Tell me when work is done. |
| Coordinate sessions | Split a large task across helper sessions, write clear briefs, and track progress. The coordinating agent merges each helper's branch into its own session branch and resolves conflicts. |

**Handoff note**

- Agents write a handoff note when the session switches to a different agent (Claude Code to Codex, or the reverse), runs out of usage, or closes. A switch inside Claude Code (for example Anthropic to DeepSeek) needs no note, because the same conversation continues.
- The app keeps the note and gives it to the next agent that works on the session.
- A copy goes in the message of the last commit on the session branch. So the note travels with the branch, and no file is added to the repo.

## Crash recovery

After a crash or restart, the IDE reopens every session that was running and resumes each one where it stopped. After a restart of the Mac, this happens when elesdee next starts: at login if the start-at-login setting is on, or else when I open the app.

- The IDE saves the state of each session while it runs: agent, preset, provider, folder, branch, and the agent's own session ID.
- On restart, the IDE relaunches each session with the same settings and resumes the agent's conversation.
- I do not need to do anything to restore my work.
- Terminal sessions cannot resume the programs that ran in them. The IDE reopens each terminal in the same folder and shows its earlier output above a new prompt.

## Providers, usage and auto-switch

I can switch provider inside a running session with one button, and the IDE can switch for me when usage runs low.

**Accounts**

- I can add more than one account per provider, for example several Claude subscriptions.
- API keys and account logins are stored in the system's own password store: the macOS Keychain in version 1, and the matching store on Windows and Linux later.
- Each account is a provider choice in a session.

**Manual switch**

- One button switches the current session to another provider or account.
- The work and context of the session carry over to the new provider. Inside Claude Code (for example Anthropic to DeepSeek), the same conversation continues. Between Claude Code and Codex, the new agent gets a handoff summary of the work so far.

**Usage panel (sidebar or tab)**

Shows every account at the same time:

| Account type | What the panel shows |
| --- | --- |
| Claude Code (each subscription) | Usage remaining for each limit of the plan (5-hour and weekly); context remaining in the current session |
| Codex | Usage remaining for each limit of the plan (5-hour and weekly, where the plan has them); context remaining in the current session |
| DeepSeek (through Claude Code) | Account balance from the DeepSeek API, so I know when to top up; context remaining in the current session |

**Allowed providers per session**

- For each session, I choose which providers and accounts it may use.
- Auto-switch uses only providers from this list.

**Auto-switch**

1. The IDE watches the usage of the current provider.
2. When usage nears its limit, the IDE switches the session to an allowed provider that has usage left (see the rules below).
3. If no allowed provider has usage left, the IDE tells the agent to commit all its work to the session branch, write a handoff note, and push the branch to the remote.
4. The IDE then pauses the session. A paused session keeps its worktree and stays in the sidebar, marked as paused. This is not a Close.
5. The session resumes by itself when an allowed provider has usage again. I can also click Resume at any time.

**Where usage numbers come from**

- The app uses only official sources for usage numbers. Where a provider has no official source, the usage panel hides that number.
- Without a number, auto-switch acts when the agent reports that it hit a limit.

Official sources:

- Claude Code: the JSON that Claude Code gives a status line script has `rate_limits.five_hour` and `rate_limits.seven_day`, each with `used_percentage` ("from 0 to 100") and `resets_at` (Unix seconds) ([Claude Code docs](https://code.claude.com/docs/en/statusline)).
  - The app sets the status line command at launch through `--settings`. The script passes the JSON to the core service.
  - Limits: the field "appears only for claude.ai Pro, Max, and Ultra subscribers", only after the first reply in a session, and each window may be absent ([Claude Code docs](https://code.claude.com/docs/en/statusline)). So the numbers exist only while a session runs on that account. At other times, the usage panel shows the last numbers it read and when it read them.
- Codex: the app-server request `account/rateLimits/read` and the `account/rateLimits/updated` notification. Each window has `usedPercent`, `windowDurationMins` and `resetsAt` ([Codex docs](https://learn.chatgpt.com/docs/app-server)).
  - The docs do not name the windows, so the app tells them apart by length: 300 minutes is the 5-hour window, 10,080 minutes is the weekly window.
  - "Pro plans currently have no five-hour limit" ([Codex docs](https://learn.chatgpt.com/docs/pricing)).
- DeepSeek: `GET https://api.deepseek.com/user/balance` returns `total_balance` and other balance fields ([DeepSeek docs](https://api-docs.deepseek.com/api/get-user-balance)).

Ruled out, because they are not official: the private usage endpoints that some third-party tools call with the user's login token, and parsing the agents' session files.

Rules from the providers:

- Anthropic: "developers may not collect, store, or intermediate Claude.ai credentials or session tokens" ([Claude Code docs](https://code.claude.com/docs/en/legal-and-compliance)), and "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products" ([Claude Code docs](https://code.claude.com/docs/en/agent-sdk/overview)). elesdee runs the user's own, unchanged Claude Code, never handles the Claude login, and does not use the Agent SDK to run sessions.
- OpenAI: "If you've built a local or open-source application using Codex app-server authentication, you can continue using it", but "App-server authentication has never been permitted for commercial or hosted services" ([Codex docs](https://learn.chatgpt.com/docs/app-server)). elesdee is local and open source.

**Auto-switch rules**

- Subscriptions (Claude, Codex): "near the limit" means 90% of any limit of the plan, the 5-hour limit or the weekly limit, whichever comes first. The margin leaves room for the agent to commit and hand off.
- DeepSeek: each account has a minimum balance that I set, for example 5 USD. Below it, auto-switch moves the session away and the app tells me to top up.
- Order: auto-switch picks subscription accounts first, then pay-per-use accounts such as DeepSeek. Among subscription accounts, it picks the one with the most usage left.

## Projects

A project is a label that groups sessions.

- Creating a project is fast: one action and a name.
- I can drag a session from one project to another.
- A project can store a default source (repo and branch) and a default preset. New sessions in the project start with these filled in, and I can change them for each session.

## Launching a session

When I launch a session, I choose where the code comes from, and the IDE gets the latest code first.

**Sources**

- A local Git branch
- A remote branch on any Git host that Git can fetch from and push to (GitHub, GitLab, Bitbucket or self-hosted)
- A plain folder (no Git)

**Git access**

- The app uses the Git credentials and SSH keys already on my Mac, the same way Git does in a terminal. There is no Git or GitHub login inside elesdee.

**Auto-sync at launch**

- From a remote branch: the IDE gets the latest version of that branch.
- From a local branch that tracks a remote branch (for example `main`): the IDE pulls the latest changes before the agent starts.
- The session starts from the latest commit, in its own new worktree, so the IDE never changes my own checkout.
- If my checkout has uncommitted changes, the launch form asks me: copy them into the new session, or leave them out.

**Branch and worktree per session**

- Every new session gets its own Git branch when it is created.
- The branch name is `elesdee/<project>/<name>`, for example `elesdee/web/fix-login`. The short name comes from the session's first prompt. Both parts are written in lowercase, with dashes in place of spaces.
- Every session gets its own Git worktree, so sessions never change each other's files.
- Worktree folders live in elesdee's own data folder on the Mac, outside the repo, so my editor, file watchers and test runners in the repo do not see them.
- Before a session closes or pauses, the agent pushes its branch to the remote.

## Demo servers

A session can start a demo server to show me its work, for example a page to review and give feedback on, or the visual output of the code base. I must be able to open it in a browser on my phone, wherever I am. On the Mac, I open `localhost` directly.

**Main way: forwarding through the phone app**

- The phone app opens the same port on the phone, for example `localhost:3000`, and forwards it over the paired iroh connection to that port on the Mac. Any browser on the phone can then open `http://localhost:3000`.
- Because the port is the same, links, live reload and cookies work as they do on the Mac. If the port is already in use on the phone, the app picks another port and shows me the address.
- The iroh team (n0) makes a tool, `dumbpipe`, that forwards a local web server port to another machine over iroh in the same way ([dumbpipe](https://github.com/n0-computer/dumbpipe)).
- Browsers treat `http://localhost` as a secure context ([MDN](https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts)), so pages that need HTTPS-only features still work.
- This needs no Tailscale. It works on the same Wi-Fi and through the relay.
- Risk: the forwarding works only while the phone app runs. Android may stop the app in the background when I switch to the browser. The test build must check this.

**Optional: Tailscale**

If Tailscale runs, the IDE also gives each demo server a Tailscale address. This helps for devices that do not have the elesdee app, such as another computer or a tablet, and for sharing with other people.

1. The IDE checks if Tailscale runs. `tailscale status --json` returns a `BackendState` field, which is `Running` when Tailscale is up ([check](https://alexwlchan.net/til/2025/check-tailscale-is-running/)). The same output gives the machine's Tailscale name ([forum](https://forum.tailscale.com/t/find-out-tailnet-name-from-script/5271)).
2. If Tailscale runs, the IDE runs `tailscale serve <port>` for each demo server. This gives an HTTPS address that only devices in my Tailscale network can open ([Tailscale docs](https://tailscale.com/docs/use-cases/application-testing/share-local-dev-server-with-team)).
3. If Tailscale is not installed or not running, demo servers use only the forwarding through the phone app.
4. The IDE tells the agent which addresses to use, so links the agent prints work on my phone.

To share a demo with people outside my Tailscale network, Tailscale Funnel can make it public ([Tailscale docs](https://tailscale.com/docs/use-cases/application-testing/share-local-dev-server-with-internet)). The IDE does this only when I ask.

## Mobile companion app

The mobile app is the only way to control the IDE remotely. It is for iOS and Android. The Android app ships first (see Releases and updates). There is no web interface. The app talks to the desktop app in real time.

**A thin shell: the UI comes from the Mac as a package**

- The phone app is a small shell. It holds only what must be native: QR pairing, the list of paired Macs, the iroh connection, the screen cache, and the native features the screens need (file picker for uploads, notifications while the app is open, demo-server forwarding, and the Android self-updater).
- The phone's UI comes from the desktop app as a package. The desktop window and the phone screens are one web UI, built with React, with two layouts: desktop and phone.
- The phone shows one Mac at a time. The shell lists the paired Macs; I pick one, and its screens open. This keeps the shell thin, because each Mac can run a different version.
- Each Mac holds the UI package of its own version. On connect, the phone compares that version with its stored copy for that Mac. If the Mac has a new version, the phone downloads the whole package once and stores it.
- The UI is not streamed. It runs from the stored package on the phone. Only data goes over the connection: session output, my input, and events.
- So after I update the desktop app, the phone gets the new UI the next time it connects. The phone app needs no update.
- The shell shows the stored UI package through a Tauri custom protocol, which serves web pages from the app's own code ([Tauri docs](https://docs.rs/tauri/latest/tauri/struct.Builder.html)).
- The shell downloads UI packages only from paired Macs, over the authenticated connection. The UI cannot load code from the internet.
- The shell offers its native features to the screens through a small bridge with its own version number. If a Mac's screens need a newer bridge, the phone tells me to update the phone app.

**Store rules for a UI package downloaded from the Mac**

- Google Play: an app may not download executable code from a source other than Google Play, but this does not apply to code that runs in an interpreter, "such as JavaScript in a webview or browser" ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/9888379)). So the thin shell is allowed on Android, from GitHub and from Google Play.
- Apple: guideline 2.5.2 says apps may not "download, install, or execute code which introduces or changes features or functionality of the app". Guideline 4.2.7 allows remote desktop clients only when the software runs and renders on the host, and limits them to a user-owned host on a local network ([Apple](https://developer.apple.com/app-store/review/guidelines/)). A UI package downloaded from the Mac over the internet does not clearly fit, so App Store and TestFlight review are a risk for the iPhone app. This is part of the later iPhone decision.

From the mobile app I can:

- See all running sessions, updated in real time.
- Type and send input to any session.
- Upload files into a session. An uploaded file goes in elesdee's own folder for that session, outside the repo, and the agent is told its path. Nothing is added to the repo unless the agent copies it in.
- View what a session creates: files, images, diffs and running demo servers. A diff shows all the session's changes since it branched off, committed or not, like a pull request view. The Mac shows diffs the same way.
- Start new sessions: pick a preset, project and source, and type the first prompt.
- Close sessions, with the same confirmation as on the Mac.
- Switch provider with the one-button switch, and see the usage panel.
- Approve agent permission prompts and new sessions that agents start, and answer "Ask me" questions.

The phone app has no lock of its own. The phone's screen lock is enough.

**Notifications on the phone**

- The phone shows notifications and questions from sessions only while the elesdee app is open. There are no push notifications when the app is closed.
- When I open the app, it shows what I missed. The desktop always shows notifications.

**What triggers a notification**

- The app sends a notification only when a session is waiting for me: it needs input, a permission answer, or an approval.
- Agents can also send one with the Notify me tool.
- Other events (an agent's turn ended, a provider switch or pause, errors) show in the app but do not send a notification.

**Pairing (one time)**

1. The desktop app shows a QR code.
2. The phone scans it. The QR code holds the desktop app's identity key, the relay server address and a one-time pairing secret.
3. The two devices save each other's keys. From then on, each knows the other is genuine.

**After pairing**

- The phone connects to the desktop app from any network, with no more setup.
- The desktop app lists paired devices. I can remove a device at any time.
- Pairing is many to many. One Mac can pair with several phones, and one phone can pair with several Macs. The phone app lets me pick which Mac to control.

## Connection technology

Recommendation: iroh. Each device is reached by its public key. iroh tries a direct link first and falls back to a relay when the network blocks it ([iroh docs](https://iroh.computer/docs/overview)). Fallback plan: WebRTC data channels.

**How it works**

1. At pairing, the phone gets the desktop app's public key from the QR code.
2. The phone connects to that key. The relay server helps the two devices find each other.
3. iroh then tries a direct link. If the network blocks it (common on mobile data), traffic stays on the relay.
4. All traffic is encrypted end to end. The relay cannot read it ([iroh docs](https://docs.iroh.computer/languages/wasm-browser)).

**Relay server**

- The relay does the jobs that WebRTC splits across three services: introduction, address discovery and backup relay.
- Version 1 uses iroh's public relays. n0.computer runs them and they are free to use ([iroh docs](https://docs.iroh.computer/iroh-services/relays/public.md)). Users cannot change the relay in the app.
- The public relays have limits. Traffic is rate-limited, there is no uptime guarantee, and n0.computer can drop support for older iroh versions at any time. The relays can see connection metadata, such as IP addresses, connection times and the amount of data, but not the content. iroh says they are for development and hobby use only ([iroh docs](https://docs.iroh.computer/iroh-services/relays/public.md)).
- iroh accepts a custom list of relay servers ([iroh docs](https://docs.rs/iroh/latest/iroh/endpoint/enum.RelayMode.html)). The list ships inside the app, so a later app update can move all users to another relay.
- We stay on the public relays until users see slow or dropped connections. Then an app update moves everyone to one of the options below.
- Options for that move:
  - iroh Shared Relays, run by n0.computer: 19 USD per month for up to 10,000 connections at a time and 100 GB of relay traffic per month, with a 5 MB/s rate limit ([iroh docs](https://docs.iroh.computer/iroh-services/relays/shared.md)).
  - Our own server running iroh's open-source relay program, `iroh-relay` ([iroh docs](https://docs.iroh.computer/iroh-services/relays/self-hosted.md)).

**Same Wi-Fi**

- When the phone and the desktop are on the same local network, they connect directly, with no relay.
- iroh's optional mDNS lookup announces each device on the local network and finds the other, so a connection can be made directly without a relay ([iroh docs](https://docs.iroh.computer/connecting/local-address-lookup.md)). It is a separate crate and is not on by default, so we must add it.

**Mobile shell (Tauri mobile)**

- One small Rust library holds iroh and our message format. The desktop core service and the phone shell both use it, so pairing, messages and reconnection are written once.
- The phone shell is built with Tauri mobile. Tauri's core is Rust, so the shell uses this library directly, with no bridge to another language.
- Why Tauri mobile is mature enough for this shell:
  - Tauri 2.0 has been stable since October 2024 and builds apps for iOS and Android ([Tauri blog](https://v2.tauri.app/blog/tauri-20/)).
  - The shell needs few native features, and Tauri's official plugins cover them on Android and iOS: QR scanning ([Barcode Scanner](https://v2.tauri.app/plugin/barcode-scanner/)), notifications ([Notification](https://v2.tauri.app/plugin/notification/)) and file picking ([Dialog](https://v2.tauri.app/plugin/dialog/); no folder picker on mobile). Demo-server forwarding and the screen cache are plain Rust.
  - The screens are web pages, so React Native's native UI parts would not be used.
- Known gaps:
  - At the 2.0 release, Tauri said not all official plugins support mobile, and that it was not happy with the mobile developer experience yet. Its GitHub Action did not build mobile apps yet ([Tauri blog](https://v2.tauri.app/blog/tauri-20/)). So our release workflow may need to call the Tauri CLI for the Android build.
  - The updater plugin is set up for desktop targets only ([Tauri docs](https://v2.tauri.app/plugin/updater/)). The Android self-updater is our own code.
- Fallback: if the test build finds a blocker in Tauri mobile, the shell moves to React Native with a web view. iroh then needs a bridge to React Native: uniffi-bindgen-react-native, a tool from Mozilla ([Mozilla](https://hacks.mozilla.org/2024/12/introducing-uniffi-for-react-native-rust-powered-turbo-modules/)), a route the iroh team names as one that works ([iroh blog](https://iroh.computer/blog/ffi-updates)).
- The test build must run the shell on a real Android phone. The iPhone test comes when the iPhone app starts.

**iroh compared with WebRTC**

| Question | iroh (recommended) | WebRTC data channels (fallback) |
| --- | --- | --- |
| Servers to run | None at first (iroh public relays); later one relay server or a paid relay service | Rendezvous server, STUN, TURN relay |
| Pairing | QR code holds one public key | QR code holds keys and server details; an offer/answer exchange on each connect |
| Mobile libraries | Official Swift and Kotlin ([iroh docs](https://docs.iroh.computer/languages)) | Mature native WebRTC libraries |
| Maturity | Version 1.0 since June 2026 ([Pinggy](https://pinggy.io/blog/iroh_1_0_dial_keys_not_ips/)) | Used in production for many years |

**Risks**

- iroh is new, so there is less real-world use behind it.
- The iroh team paused its mobile libraries in 2025 ([iroh blog](https://iroh.computer/blog/ffi-updates)). They are official again now, but we must test them on real phones.
- Tauri mobile is younger than React Native. The test build checks it before we commit to it.
- The public relays are rate-limited and have no uptime guarantee. If n0.computer drops support for an old iroh version, users who do not update lose the relay. Direct connections on the same Wi-Fi still work.

**First step: a test build**

Build this test before the real app.

1. Build the Tauri mobile shell, pair an Android phone and the desktop app by QR code, and download the UI package from the desktop.
2. Stream a live session over iroh, with the phone's message view fed by the Claude Code hooks and by a second client on the Codex app-server (see Where the message view gets its data).
3. Test on Wi-Fi and on mobile data.

If iroh works well, commit to iroh; if not, switch to WebRTC. If Tauri mobile has a blocker, switch the shell to React Native.

**Design rules**

- Keep the app's messages separate from the transport, so a switch from iroh to WebRTC does not change the app.
- Both sides prove their identity with the keys saved at QR pairing.

## Releases and updates

The desktop app and the mobile apps are published through GitHub Releases in this repository. The repository is public, so apps can download releases without a GitHub login. An update installs over the existing app and keeps all its data: sessions, presets, accounts, paired devices and browser logins.

**Release builds**

- A GitHub Actions workflow builds and publishes each release. The Git tag of the release sets the version number.
- There are only stable releases: no beta and no nightly builds. Every release goes to every user.
- The signing keys are stored as GitHub Actions secrets, never in the repository.
- The core service is bundled inside the desktop app as a sidecar binary ([Tauri docs](https://v2.tauri.app/develop/sidecar/)). One update replaces the window and the core service together, so the two always have the same version.

**Desktop app (macOS)**

- The desktop app runs on Apple Silicon Macs only. There is no Intel build.
- It supports the two latest major versions of macOS.
- Tauri's updater plugin reads a static JSON file from the latest GitHub release, for example `https://github.com/<owner>/<repo>/releases/latest/download/latest.json` ([Tauri docs](https://v2.tauri.app/plugin/updater/)).
- The `tauri-action` GitHub Action builds the app, signs the update files and uploads this JSON file to the release ([tauri-action](https://github.com/tauri-apps/tauri-action)).
- The app checks the signature of each update before it installs it. Tauri does not allow this check to be turned off ([Tauri docs](https://v2.tauri.app/plugin/updater/)).
- By default, macOS Gatekeeper checks that an app downloaded from outside the App Store is signed by a registered developer and notarized by Apple ([Apple](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web)). This needs a Developer ID from the Apple Developer Program, which costs 99 USD per year ([Apple](https://developer.apple.com/programs/)).
- I will join the Apple Developer Program before the first release, so every release is signed and notarized.

**Update checks (desktop)**

- The app checks GitHub for a new release at launch and every few hours.
- When it finds one, it downloads it in the background and verifies its signature.
- It then tells me that an update is ready. It installs the update only when I click Install.

**Update while sessions run**

- An update restarts the core service. When I click Install, the app uses the same steps as quit: it saves every session, stops the core service, installs the update, starts again and resumes the sessions (see Crash recovery).
- An agent that is mid-task is interrupted. After the restart, it resumes its conversation.
- Tauri does not force a restart right after it installs an update. The app chooses when to restart ([Tauri docs](https://v2.tauri.app/plugin/updater/)).

**Android app**

The Android app ships on GitHub first and moves to Google Play later.

- Each GitHub release has the signed APK file attached.
- Android installs an update over the existing app only if the update is signed with the same key ([Android docs](https://developer.android.com/studio/publish/app-signing)). So the signing key must never change. If the key is lost, no update can install over the existing app.

*GitHub phase*

- The app updates itself. It checks GitHub for a new release of the shell, downloads the APK, and asks Android to install it over the old app. Android asks the user to confirm.
- Shell updates are rare: a new UI comes as a package from the desktop app, not from an app update.
- To install an APK, an app that targets API level 26 or newer must hold the `REQUEST_INSTALL_PACKAGES` permission ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/12085295)).
- Users who prefer it can use Obtainium, an Android app that installs and updates apps from their GitHub release pages ([Obtainium](https://github.com/ImranR98/Obtainium)).

*Google Play phase*

- An app distributed on Google Play may not update itself by any method other than Google Play's update mechanism ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/9888379)). Self-update is also not on Play's list of allowed uses for the install permission ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/12085295)).
- So the build for Google Play leaves out the self-updater and the install permission. The GitHub build keeps them.
- Play App Signing lets a developer upload an existing app signing key, with Google's PEPK tool ([Play Console Help](https://support.google.com/googleplay/android-developer/answer/9842756)). We upload the key that signs the GitHub builds, so Play updates install over GitHub installs.

*Developer verification (both phases)*

- Google now verifies the identity of Android developers. From 30 September 2026, the protections begin for users in Brazil, Indonesia, Singapore and Thailand who install apps from participating stores on certified devices. In 2027, Google plans to expand verification worldwide to all apps on certified devices ([Android docs](https://developer.android.com/developer-verification)).
- Users can still install apps from unverified developers through a one-time "advanced flow", which includes a one-day wait. A free "limited distribution" account lets a developer share an app with up to 20 devices, with no ID and no fee ([Android blog](https://android-developers.googleblog.com/2026/03/android-developer-verification.html)).
- Decision: I register as a verified Android developer before the first public Android release, so users install with no extra steps.

**iPhone app**

iOS does not install an app from a GitHub download. Apple's routes:

| Route | Who can install | Limits |
| --- | --- | --- |
| TestFlight | Up to 10,000 external testers ([Apple](https://developer.apple.com/testflight/)) | The first build needs App Review ([Apple](https://developer.apple.com/testflight/)). Each build stops working after 90 days ([Apple](https://developer.apple.com/help/app-store-connect/test-a-beta-version/testflight-overview/)). |
| Ad hoc | Only devices registered in the developer account | 100 iPhones per membership year ([Apple](https://developer.apple.com/help/account/devices/devices-overview/)) |
| App Store | Anyone | Full App Review |
| Web Distribution (EU only) | Users in the EU | The developer must meet one of Apple's criteria, for example one million first annual installs worldwide or a financial-stability score ([Apple](https://developer.apple.com/support/dma-and-apps-in-the-eu/)) |

- All of these routes need the Apple Developer Program.
- Decision: the Android app ships first. The iPhone route is decided later.

**Windows and Linux (after version 1)**

Facts to plan for:

- Web view: Tauri uses WebView2 on Windows and WebKitGTK (`webkit2gtk`) on Linux ([Tauri docs](https://v2.tauri.app/reference/webview-versions/)). The window must be tested on both.
- Updates on Windows: Tauri quits the app before it installs an update, because of a limit of Windows installers ([Tauri docs](https://v2.tauri.app/plugin/updater/)). So the core service must save all sessions before the install starts.
- Updates on Linux: the updater page describes updates only for the AppImage format ([Tauri docs](https://v2.tauri.app/plugin/updater/)). Tauri can also build Debian, RPM, Snap, Flatpak and AUR packages ([Tauri docs](https://v2.tauri.app/distribute/)), but those would update through their own package managers.
- Signing on Windows: an unsigned app gets a SmartScreen warning. Tauri documents OV certificates, Azure Key Vault and Azure Artifact Signing (previously Azure Trusted Signing). Since 2024, an EV certificate no longer gives instant SmartScreen trust, and an OV certificate is cheaper and available to individuals ([Tauri docs](https://v2.tauri.app/distribute/sign/windows/)).
- Azure Artifact Signing: individual developers must be located in the United States or Canada ([Microsoft](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)).
- Claude Code runs natively on Windows or inside WSL. Git for Windows is recommended, so Claude Code can use its Bash tool ([Claude Code docs](https://code.claude.com/docs/en/setup)).
- Codex on Windows: not checked yet.

**Version numbers**

- The phone's UI package comes from the desktop app, so it always has the desktop app's version.
- The phone shell has its own version number. A new shell is released only when its native features or its bridge change, so users are not asked to update a shell that did not change.
- Both are built by the same release workflow. A release that changes the shell attaches the new APK.
- The message format between the phone and the desktop carries a version number, and the bridge has its own version number. When the shell is too old for a Mac's screens, the phone shows "update the phone app". When a Mac is too old for the shell, the phone shows "update elesdee on this Mac".

## Open questions

**Scope**

- [ ] Windows and Linux: which Linux packages, which Windows signing route, and does Codex run natively on Windows?

**Releases and updates**

- [ ] Which route for the iPhone app, when it ships: TestFlight, ad hoc or App Store? Apple's rules on downloaded code are a risk for a shell that downloads its UI package from the Mac (see Store rules).
