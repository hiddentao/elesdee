# AGENTS.md

This repository holds the elesdee product spec, research notes, clickable UI mockups and the application.

This file carries the rules that bind every prompt. Detail and decisions live in `.dev/`.

## Where things are

| Path | Contents |
| --- | --- |
| `.dev/SPEC.md` | The product spec. The source of truth for behavior. |
| `.dev/api.md` | The core service API contract: types and endpoints. |
| `.dev/hooks.md` | Agent hook integration and the permission answer flow. |
| `.dev/mockups/round-1.html` | The clickable Mac and phone mockups. A is dark, B is light. The visual reference for the app. |
| `.dev/research/` | Research notes behind the spec. |
| `core/` | The Rust core service: local HTTP API, sessions, event stream. |
| `app/` | The React and TypeScript desktop UI, built with Vite. |
| `src-tauri/` | The Tauri shell that starts the core service and shows the UI. |
| `.github/workflows/ci.yml` | CI: UI checks and build, core format/clippy/tests, and a macOS shell check. |
| `AGENTS.md` | These rules. |

## Current phase

The application has its first working slice. The core service runs real PTYs, projects, accounts, Git branches and worktrees, persistence and a secured local API. The desktop UI runs real terminals and agent CLIs, creates, renames, closes, reopens and deletes sessions, adds projects, shows live previews and usage, and answers Claude Code permission prompts from a card. The Tauri shell starts the core and embeds the UI.

Not built yet: hook adapters for Codex and OpenCode 2 (only Claude Code sends permission prompts to the app), usage-based auto-switch, the phone shell, the full settings page and provider add flow, and packaging. The spec describes the target; the mockup is the visual reference.

## Running the app

- Core service: `npm run core` (or `cargo run --manifest-path core/Cargo.toml`). It listens on `127.0.0.1:4319`.
- UI in a browser: `npm run dev`. Vite proxies `/api` to the core.
- Desktop shell: `npm run desktop`. It starts the core if none runs and loads the UI.

## Releases

- Run the **Release** workflow from `main`; it opens a `release/<version>` PR that bumps every version file and writes `CHANGELOG.md`. The PR is gated on CI for the current `main` commit.
- Merging the release PR runs **Release Publish**: it tags the merge commit (no `v` prefix), builds the Apple Silicon macOS app and the core binary, and publishes the GitHub Release.
- `scripts/release.mjs` is the version and changelog tool. `node scripts/release.mjs --dry-run` previews the next version.
- Set the repository variable `RELEASE_APP_ID` and the secret `RELEASE_APP_PRIVATE_KEY` so the release PR triggers its own CI. Without them the PR opens with `GITHUB_TOKEN` and its CI does not start.
- The macOS build is unsigned. It will be signed and notarized when the Apple Developer certificate secrets are added.

## Commit gates

- **Conventional commits.** `commitlint` checks every commit message through `.husky/commit-msg`.
- **`Docs-Checked:`** is required when a commit changes `core/src/`, `app/src/`, `src-tauri/src/`, `scripts/` or `.github/workflows/`. The value names the docs updated or verified, or why none apply.
- **`Design-Checked:`** is also required when a commit changes `app/src/components/`, `app/src/styles/`, `app/src/App.tsx` or `app/index.html`. The value names the mockup, theme and width checks done.
- Release and merge commits are exempt. `HUSKY=0` is set only for generated release commits in CI.
- The checker is `scripts/check-commit-acks.mjs`; its tests run with `npm run acks`.

## Keep this file and `.dev/` current

- **Every change updates the documents it affects, in the same commit.** That includes `.dev/SPEC.md` and this file.
- **Rules stay here. Detail goes in `.dev/`.** Link the detail; do not repeat it.
- **A fact in more than one place changes in every place in the same commit.** The mockup, the spec and these rules must agree.
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
- **Interface copy never names the operating system.** Say "this computer", the "system keychain" or the desktop window, never "this Mac" or "macOS". Version 1 runs on macOS; Windows and Linux follow.

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
