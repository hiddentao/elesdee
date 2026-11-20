# AGENTS.md

This repository holds the elesdee product spec, research notes, clickable UI mockups, the public product docs and the application.

This file carries the rules that bind every prompt. Detail and decisions live in `.dev/`.

## Where things are

| Path | Contents |
| --- | --- |
| `.dev/SPEC.md` | The product spec. The source of truth for behavior. |
| `.dev/api.md` | The core service API contract: types and endpoints. |
| `.dev/hooks.md` | Agent hook integration and the permission answer flow. |
| `.dev/mockups/round-1.html` | The clickable Mac and phone mockups. A is dark, B is light. The visual reference for the app. |
| `.dev/research/` | Research notes behind the spec. |
| `docs/` | Public product documentation. The website build starts from this folder at release. |
| `core/` | The Rust core service: local HTTP API, sessions, event stream. |
| `app/` | The React and TypeScript desktop UI, built with Vite. |
| `app/src/i18n/` | The interface language catalogs, resolver and provider. |
| `src-tauri/` | The Tauri shell that starts the core service and shows the UI. |
| `.github/workflows/ci.yml` | CI: UI checks and build, core format/clippy/tests, and a macOS shell check. |
| `AGENTS.md` | These rules. |

## Current phase

The application has its first working slice. The core service runs real PTYs, projects, accounts, four agent clients, Git branches and worktrees, persistence and a secured local API. It detects Claude Code, Codex, OpenCode 2 and Google Antigravity (`agy`). Accounts cover the supported providers with subscription, API-key and local-server sign-in, and custom OpenAI- or Anthropic-compatible endpoints; keys live in the system keychain and are redacted from session output. A fresh install starts with no projects and sample accounts marked as samples. The desktop UI runs real terminals and agent CLIs, creates, renames, closes, reopens and deletes sessions, adds projects, shows live previews and usage, starts sessions from a chosen branch with an optional first prompt and per-session environment overrides, switches a session to another account, lists and answers agent questions, approves or denies agent-started sessions, and answers Claude Code, Codex and OpenCode 2 permission prompts from a card. It asks before quitting while sessions run, and restores running sessions as paused after a restart. Usage near a limit switches a session to an account with room, pauses it when none has room, and resumes it when one does. Claude Code, Codex and OpenCode 2 sessions get the elesdee MCP server; Claude Code and OpenCode 2 also get the elesdee skill. The tools ask questions, notify, report done, read other sessions, send messages, check usage, withdraw questions and request new sessions for approval. The interface comes in 12 languages from catalogs in `app/src/i18n/`, chosen on the Languages page in Settings; the choice follows the computer language on first launch. A phone layout at `/phone.html` shows the session list, one session tab at a time with a numbered tab sheet, read-only terminals, permission cards and questions, in the phone's own system language. The Tauri shell starts the core, embeds the UI, asks before quitting and opens a native folder picker.

Not built yet: launch presets and project defaults, the Tauri phone shell with pairing and the remote connection, the elesdee skill for Codex, the built-in browser and file editor, demo servers, and the phone's own Languages setting. Packaging builds the macOS app and signs and notarizes it when the Apple secrets are set; without them the build is ad-hoc signed. The spec describes the target; the mockup is the visual reference.

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
- **Open-ended dropdowns must be filterable combo boxes with asynchronous loading.** Never use a native `<select>` or a fixed list for values that can grow without a known bound: projects, branches, folders, accounts, sessions, files, servers and similar. The user types to filter. Options load asynchronously with a visible loading state. Keep keyboard navigation, focus handling and screen-reader labels.
- **The mockup is self-contained.** No network calls, webfonts or external assets. It is a prototype, not app code.
- **Usage belongs to the Mac, not to a session.** The sidebar summary opens the global usage panel. A session's side panel holds Files, Changes and Browser only.
- **Each session has its own browser context.** Tabs, cookies, storage and logins never cross sessions.
- **The user closes a session.** An agent can report done, but cannot close.
- **A fresh install starts with no projects.** Seed accounts may exist, marked as samples with no usage data. The first-launch screen adds the first project.
- **Permission cards are for mutating tools only.** Claude Code, Codex and OpenCode 2 send approval requests to the app. The Claude matcher covers `Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch`. Codex uses its app-server approval requests on the websocket endpoint. OpenCode uses the permission list on its session server. Read-only tools, `AskUserQuestion` and `ExitPlanMode` stay in the TUI.
- **Antigravity keeps its own prompts.** The `agy` terminal handles permission prompts. Do not add a Claude hook settings file or a permission card for it. Its session gets the base keys only.
- **API keys live in the system keychain.** Account metadata may be saved. Never write a key to `state.json`, an API response, a log or a session output.
- **Interface copy never names the operating system.** Say "this computer", the "system keychain" or the desktop window, never "this Mac" or "macOS". Version 1 runs on macOS; Windows and Linux follow.
- **Interface copy comes from the language catalogs.** English in `app/src/i18n/en.ts` holds the keys. Every other catalog has every key. Never leave interface text in a component. Add or change a text in all 12 catalogs in the same commit.
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
- `.github/workflows/ci.yml` runs the UI checks and build, core format, clippy and tests, and a macOS desktop-shell check on every pull request.
