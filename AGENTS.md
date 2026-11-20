# AGENTS.md

This repository holds the elesdee product spec, research notes, clickable UI mockups, the public product docs and the application.

This file carries the rules that bind every prompt. Detail and decisions live in `.dev/`.

## Where things are

| Path | Contents |
| --- | --- |
| `.dev/SPEC.md` | The product spec. The source of truth for behavior. |
| `.dev/api.md` | The core service API contract: types and endpoints. |
| `.dev/hooks.md` | Agent hook integration and the permission answer flow. |
| `.dev/phone-shell.md` | The phone shell, pairing and remote connection design. |
| `.dev/providers-configs.md` | The provider credential and AI config model, models and effort. |
| `.dev/browser.md` | The session browser: Chromium profiles, core routes, agent tools, the Browser panel and its tests. |
| `.dev/mockups/round-1.html` | The clickable Mac and phone mockups. A is dark, B is light. The visual reference for the app. |
| `.dev/research/` | Research notes behind the spec. |
| `docs/` | Public product documentation. The website build starts from this folder at release. |
| `core/` | The Rust core service: local HTTP API, sessions, event stream, remote link endpoint and pairing. |
| `link/` | Shared Rust link crate: device identity, pairing ticket, framing, method allowlist, limits and the phone link client. The core and the phone shell use it. |
| `app/` | The React and TypeScript desktop UI, built with Vite. |
| `app/src/i18n/` | The interface language catalogs, resolver and provider. |
| `phone-shell/` | The Tauri v2 phone app for iOS and Android: link client host, keychain identity, pairing screens, computer list, live session list and output, demo-server forwarding and its list, and its own 12-language catalogs. |
| `src-tauri/` | The Tauri shell that starts the core service and shows the UI. |
| `.github/workflows/ci.yml` | CI: UI checks and build, core and link format/clippy/tests, phone shell catalogs, types, build and Rust checks, end-to-end tests, and a macOS desktop-shell check. |
| `AGENTS.md` | These rules. |

## Current phase

The application has its first working slice. The core service runs real PTYs, projects, providers, AI configs, four agent clients, Git branches and worktrees, persistence and a secured local API. It detects Claude Code, Codex, OpenCode 2 and Google Antigravity (`agy`). Providers cover the supported vendors with subscription, API-key and local-server sign-in, and custom OpenAI- or Anthropic-compatible endpoints. Subscription sign-in for Anthropic and OpenAI starts the installed client's own login, follows the browser return or a one-time code, and asks the client to confirm the credential before it adds the provider; the client keeps the token and the provider records only that the sign-in is connected. Keys live in the system keychain and are redacted from session output. A fresh install starts with no projects and sample providers and configs marked as samples. The desktop UI runs real terminals and agent CLIs, creates, renames, closes, reopens and deletes sessions, adds projects, shows live previews and usage, starts sessions from a chosen branch with an optional first prompt and per-session environment overrides, stores a default AI config and source branch for each project and prefills new sessions from them, switches a session between AI configs, and adds, edits and removes providers and AI configs in Settings, lists and answers agent questions, approves or denies agent-started sessions, answers Claude Code, Codex and OpenCode 2 permission prompts from a card, and sends a desktop notification when a session needs an answer, reports done or sends a message; a click on the notification opens that session. A launch fetches a remote branch or a tracked local branch first, starts the session from the latest commit without changing the user's checkout, stops when the local and remote branches have diverged or the fetch fails, and offers to copy uncommitted changes into the new worktree for that launch only. Each session has a side panel that lists its files, opens a text file in a CodeMirror editor with an explicit save, and shows changed files with counts and diffs; the Files tab also lists files an agent shares, with a text or image preview and a download, and lists the session's registered demo servers with their local address, state and Stop action. It asks before quitting while sessions run, and on the next start it restarts every session that was running and resumes the agent's own conversation by the saved session ID. AI configs name a provider and a client and add a model, effort, extra arguments, environment overrides and config overrides. Settings lists, adds, edits and deletes them, and the launch form picks any config as a filterable list that keeps the provider usage visible. The config form loads the provider model list and accepts a typed custom model name. Usage comes from official sources: Claude Code's status line while a session runs, Codex's app-server rate-limit read and updates, and the DeepSeek balance endpoint; each reading stores the window kind, the remaining values and the read time, marks a failed read as stale or unknown, and other providers show no usage data. Clicking the provider pill opens the config chooser, which lists the AI configs allowed for the session with their provider usage, marks the config the automatic switch would use next, and edits the allowed list. The pill tooltip shows the session's context left; it comes from the Claude Code status line and stays unknown for the other clients. A session stores the AI configs it may use, chosen at launch and editable from the switch chooser while it runs. Usage near a limit switches a session to an allowed config: subscription providers first, then pay-per-use providers, the most room first, and the current client on a tie; a switch to another client carries the handoff note, and the session pauses when no allowed config has room, then resumes when one does. Agents save a handoff note through a tool; a switch to a different client gives the note and the end of the log to the new agent, a usage pause keeps it, and Close puts it in the last commit on the session branch without adding a file. Claude Code, Codex and OpenCode 2 sessions get the elesdee MCP server and the same seven skills: elesdee, Demo servers, Handoff, Ask and notify, Coordinate sessions, Browser checks and Frontend design. Each session's skills live in a private data-directory folder that is removed when the session closes. The tools ask questions, notify, report done, write a handoff note, read other sessions, send messages, check usage, withdraw questions and request new sessions for approval, wait for another session to need attention or reach a state, share a file from the session folder with the user, and register a demo-server port. Each session can run its own Chromium browser with a separate private profile, open tabs, and let the agent open a page, click, type, read the visible text, wait and share a screenshot; the Browser tab of the side panel shows the session's tabs, one address field, Back, Forward and Reload, a loading state and a live screenshot, and a closed session refuses browser actions. Each session can own up to eight demo servers on loopback ports; the Files panel shows each address and can stop it, and a paired phone lists the active ports and forwards only a registered port over the connection. The interface comes in 12 languages from catalogs in `app/src/i18n/`, chosen on the Languages page in Settings; the choice follows the computer language on first launch. A phone layout at `/phone.html` shows the session list, one session tab at a time with a numbered tab sheet, read-only terminals, permission cards and questions. It follows the phone's system language on first launch, and its Languages sheet sets the phone language apart from the desktop choice. The Tauri shell starts the core, embeds the UI, asks before quitting and opens a native folder picker. Settings has General, Devices and Diagnostics pages: General starts the app at login and keeps the computer awake while agent sessions run, Devices lists paired phones with their platform, last seen time and live connection state and removes one with confirmation, and Diagnostics writes a redacted bundle from the local log into a folder chosen with the native picker. The desktop shell registers the login item and the keep-awake request, starts hidden when the system launches it at login, and shows the window when the user clicks the app. The core also holds the desktop link identity in a private key file, starts the iroh endpoint, runs the pairing state machine with a six-digit confirmation code, stores paired device metadata without secrets, serves the local pairing and device routes, and answers the session snapshot method to a paired phone. The shared link crate holds the phone client: it loads or creates the phone identity in the platform keychain, parses and checks the pairing ticket, runs the pairing handshake, saves the computer record, opens the app connection, sends `hello` and checks `welcome`, and exposes event and output subscriptions with bounded reconnect delays and output resume. The phone shell in `phone-shell/` is a Tauri v2 skeleton for iOS and Android with a plain TypeScript UI and the shared design tokens. It pairs by pasted ticket or QR scan, shows the six-digit code, handles refusal, expiry and cancel, lists stored computers with their connection state, shows the live session list, opens a session with its live read-only output, lists the session's active demo servers with their local address, state and Open action, and shows connection and revocation notices. It binds a `127.0.0.1` listener for each registered demo-server port, uses the same port when free or a free port otherwise, forwards each accepted socket over the forward connection to the computer's loopback port, keeps one tunnel per port and at most eight, and tears the tunnels down on disconnect, revocation and exit. Its screens use 12 own language catalogs. The generated Android and iOS projects, the downloaded UI package, the upload, image-read, UI-package and demo-server methods, and device testing are not built.


Not built yet: the desktop pairing screens that show the code, the generated Android and iOS projects for the phone shell, phone shell device testing, the downloaded phone UI package, the upload, image-read and UI-package link methods, the phone's browser handoff on a device, any background forwarding behavior, the Tailscale address for a demo server, the login-item and keep-awake behavior in a packaged, signed build, downloading Chromium on first use and encrypting the browser profile at rest, clicking and typing directly in the browser live view, the browser console and network log tools, and the setting for a DeepSeek minimum balance. Packaging builds the macOS app and signs and notarizes it when the Apple secrets are set; without them the build is ad-hoc signed. The spec describes the target; the mockup is the visual reference.

## Running the app

- Core service: `npm run core` (or `cargo run --manifest-path core/Cargo.toml`). It listens on `127.0.0.1:4319`.
- UI in a browser: `npm run dev`. Vite proxies `/api` to the core.
- Desktop shell: `npm run desktop`. It starts the core if none runs and loads the UI.

## Releases

- Run the **Release** workflow from `main`; it opens a `release/<version>` PR that bumps every version file and writes `CHANGELOG.md`. The PR is gated on CI for the current `main` commit.
- Merging the release PR runs **Release Publish**: it tags the merge commit (no `v` prefix), builds the Apple Silicon macOS app and the core binary, and publishes the GitHub Release.
- `scripts/release.mjs` is the version and changelog tool. `node scripts/release.mjs --dry-run` previews the next version.
- Set the repository variable `RELEASE_APP_ID` and the secret `RELEASE_APP_PRIVATE_KEY` so the release PR triggers its own CI. Without them the PR opens with `GITHUB_TOKEN` and its CI does not start.
- The macOS build is signed and notarized when `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID` are set. Without them it is built and signed ad hoc, and the release notes say it is unsigned.

## Commit gates

- **Conventional commits.** `commitlint` checks every commit message through `.husky/commit-msg`.
- **`Docs-Checked:`** is required when a commit changes `core/src/`, `app/src/`, `src-tauri/src/`, `scripts/` or `.github/workflows/`. The value names the docs updated or verified, or why none apply.
- **`Translations-Checked:`** is also required when a commit changes `core/src/`, `app/src/` or `src-tauri/src/`. The value names the language catalogs updated or verified, or why none apply. User-facing strings stay in sync across languages; English alone is not done.
- Release and merge commits are exempt. `HUSKY=0` is set only for generated release commits in CI.
- The checker is `scripts/check-commit-acks.mjs`; its tests run with `npm run acks`.

## Keep this file, `.dev/` and `docs/` current

- **Every change updates the documents it affects, in the same commit.** That includes `.dev/SPEC.md` and this file.
- **Rules stay here. Detail goes in `.dev/`.** Link the detail; do not repeat it.
- **Public docs live in `docs/`.** They describe what a user can do now. Internal decisions, the spec and research stay in `.dev/`.
- **A fact in more than one place changes in every place in the same commit.** The mockup, the spec, the public docs and these rules must agree.
- **A new topic starts as a `.dev/` document.** Add it to the map above.
- **The spec comes before the code.** A change in behavior updates the spec in the same commit.

## Hard rules

- **Sessions have two states: Active and Closed.** Working, waiting, paused and reported done are details inside Active. Only the user closes a session.
- **Do not add redundant interface elements or duplicate functionality.** Before adding a control, check whether the same action already exists in the same context. Keep one clear control instead of several shortcuts.
- **Open-ended dropdowns must be filterable combo boxes with asynchronous loading.** Never use a native `<select>` or a fixed list for values that can grow without a known bound: projects, branches, folders, providers, AI configs, sessions, files, servers and similar. The user types to filter. Options load asynchronously with a visible loading state. Keep keyboard navigation, focus handling and screen-reader labels.
- **The mockup is self-contained.** No network calls, webfonts or external assets. It is a prototype, not app code.
- **Usage belongs to the Mac, not to a session.** The sidebar summary opens the global usage panel. A session's side panel holds Files, Changes and Browser only.
- **Each session has its own browser context.** Tabs, cookies, storage and logins never cross sessions.
- **The user closes a session.** An agent can report done, but cannot close.
- **A fresh install starts with no projects.** Seed providers and configs may exist, marked as samples with no usage data. The first-launch screen adds the first project.
- **Permission cards are for mutating tools only.** Claude Code, Codex and OpenCode 2 send approval requests to the app. The Claude matcher covers `Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch`. Codex uses its app-server approval requests on the websocket endpoint. OpenCode uses the permission list on its session server. Read-only tools, `AskUserQuestion` and `ExitPlanMode` stay in the TUI.
- **Antigravity keeps its own prompts.** The `agy` terminal handles permission prompts. Do not add a Claude hook settings file or a permission card for it. Its session gets the base keys only.
- **API keys live in the system keychain.** Provider metadata may be saved. Never write a key to `state.json`, an API response, a log or a session output.
- **The link identity and pairing secrets never leak.** The desktop key lives in `<data dir>/link.key` with mode 0600. `state.json` holds paired device metadata only. Never log or return a device key, a pairing ticket or a pairing secret.
- **The phone gets the allowlist, not the local API.** A paired phone may call only methods in the shared link allowlist. The link reuses the local router for safe methods; it exposes no file writes or session input before the app-connection step.
- **A phone never chooses a session folder.** A session created from the phone starts in the project folder. Ignore any folder value in the remote request.
- **Interface copy never names the operating system.** Say "this computer", the "system keychain" or the desktop window, never "this Mac" or "macOS". Version 1 runs on macOS; Windows and Linux follow.
- **Interface copy comes from the language catalogs.** English in `app/src/i18n/en.ts` holds the keys. Every other catalog has every key. Never leave interface text in a component. Add or change a text in all 12 catalogs in the same commit.
- **The phone shell has its own catalogs.** English in `phone-shell/ui/src/i18n/en.json` holds the keys, and the other 11 catalogs hold every key. A test fails when a key is missing, extra or loses a placeholder. The shell follows the phone's system language. The shell screens are not the desktop app, so they do not import `app/src/i18n/`.
- **Twelve interface languages.** English, German, French, Spanish, Italian, Brazilian Portuguese, Dutch, Polish, Swedish, Simplified Chinese, Japanese and Korean. European Portuguese uses Brazilian Portuguese. Traditional Chinese uses Simplified Chinese. There are no right-to-left languages.
- **Not translated: agent output, prompts, session logs and terminal content.** The instructions and skills that elesdee gives agents stay in English. The public docs stay in English.
- **English is the default.** A test fails when a catalog misses a key, adds an extra key or drops a placeholder.

## Default skills

- **frontend-design** — load it before any UI, mockup or visual design work in this repo.

## Verification

- Open `.dev/mockups/round-1.html` in Chrome at 1440 and 390 widths. Check both directions and both devices.
- There must be no console errors and no horizontal page scroll.
- Check the changed interaction in both directions before you commit.
- For the app: run `cargo test` and `cargo clippy --all-targets -- -D warnings` in `core/`, and `npm test`, `npm run typecheck` and `npm run build` at the root.
- Open the app at 1440 and 1024 widths in both themes. No console errors, no horizontal scroll.
- A session change is checked against the real core: start it, create a terminal session, send input, read the output, close it.
- A link change runs the core and link tests. The pairing tests use real loopback iroh endpoints and need no relay. Check that a second use of a ticket, an expired ticket and a removed device are all rejected.
- For the phone shell: run `npm --prefix phone-shell run test`, `npm --prefix phone-shell run typecheck` and `npm --prefix phone-shell run build` at the root, then `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` in `phone-shell/src-tauri/`. The UI check renders the screens in Chromium at 390 width in both themes with the native bridge stubbed. It is not a device test.
- `.github/workflows/ci.yml` runs the UI checks and build, core format, clippy and tests, link format, clippy and tests, the phone shell catalogs, types, build, format, clippy and tests, the end-to-end suite, and a macOS desktop-shell check on every pull request.
- Playwright uses ports 4399 and 5199 by default. Set `ELESDEE_E2E_CORE_PORT`, `ELESDEE_E2E_WEB_PORT`, `ELESDEE_E2E_OPENCODE_PORT` and `ELESDEE_E2E_CODEX_PORT` to run beside another checkout, and set `ELESDEE_E2E_CORE_URL`, `ELESDEE_E2E_OPENCODE_URL` and `ELESDEE_E2E_CODEX_URL` to match.
