# elesdee gap audit 2

Audit target: branch `hiddentao/round-2-mockups`, commit `fba755c` (20 November 2026).
Sources: `.dev/SPEC.md`, `.dev/phone-shell.md`, `.dev/browser.md`, `.dev/providers-configs.md`,
`.dev/api.md` and the code at the target commit.

This audit lists only gaps that are confirmed by the spec and the code. Features that are built are
not repeated here.

Excluded by request: the phone shell message view and the Chromium download, both in flight.

Not counted as feature gaps: device tests, the packaged login-item and keep-awake check, and a real
in-app update with keys and a published release. These are verification or release prerequisites.
Windows, Linux and the iPhone app are later work.

## Verdict

The Mac application now covers the first milestone, provider switching, the browser, demo servers,
crash recovery, handoff, the tools and all seven skills. The remaining work is concentrated in the
phone delivery, the parts of the browser that the spec still marks as not built, a few auto-switch
and window behaviors, and small completion items.

## Ranked gaps

| Rank | Gap | Spec section | Files that would change | Size |
| --- | --- | --- | --- | --- |
| P0 | No desktop pairing screen. The core pairing routes and QR exist, but Settings only lists and removes paired phones; the user cannot start the QR/code flow from the desktop | Pairing; Settings / Devices | `app/src/components/SettingsDialog.tsx`, new pairing dialog and QR component, `app/src/api/client.ts`, `app/src/api/types.ts`, the 12 catalogs | M — 3–5 days |
| P0 | The phone UI package is not built or delivered. The shell runs its own skeleton screens; the phone layout is served only by the browser. `ui.manifest` and `ui.download` return unsupported | Mobile companion app (thin shell, UI package); Version numbers | new `app/vite.phone.config.ts`, new `scripts/package-phone-ui.mjs`, new `app/src/api/transport.ts`, new `core/src/link/ui.rs`, `core/src/link/app.rs`, desktop core start and resources, `phone-shell/src-tauri` store and protocol, phone shell loader | L — 2–3 weeks |
| P0 | No generated Android or iOS projects and no Android build. The phone shell is a Tauri skeleton without `gen/android` or `gen/ios`; CI does not build an APK | Mobile companion app; Releases / Android app; Desktop architecture (CI) | `phone-shell/gen/android`, `phone-shell/gen/ios`, `phone-shell/src-tauri/tauri.conf.json`, `.github/workflows/ci.yml`, `release-publish.yml`, signing configuration | M/L — 1–2 weeks |
| P1 | The phone cannot upload files, read files or images, or show diffs. `sessions.upload` and `files.image` return unsupported; the phone shell has no file, image or diff screen | From the mobile app | `core/src/link/app.rs`, new `core/src/link/ui.rs` for the binary path, `core/src/api.rs` upload route, `core/src/files.rs` image read, `phone-shell/ui/src` and bridge, phone layout media views | L — 2–3 weeks |
| P1 | No phone notifications. The phone shell has no notification plugin or code; it cannot notify while open, ask permission, list what was missed, or open a session from a notification | Notifications on the phone; What triggers a notification | `phone-shell/src-tauri/Cargo.toml`, `lib.rs`, `commands.rs`, `phone-shell/ui/src` (permission, waiting events, click), phone shell catalogs | M — 3–5 days |
| P1 | The browser profile is not encrypted at rest. The profile folder has private permissions only | Browser tools | `core/src/browser/` profile handling, `core/src/secrets.rs` or the system keychain, `core/Cargo.toml` | M — 3–5 days |
| P1 | No browser console or network log tools for agents | Browser tools | `core/src/browser/`, `core/src/mcp.rs` tool list and dispatch, shared file or result types | M — 3–5 days |
| P1 | The browser live view cannot be clicked or typed in by the user; it shows only the latest screenshot | Browser tools | `core/src/browser/` input over the DevTools protocol, `core/src/api.rs` routes, `app/src/components/BrowserPanel.tsx`, `app/src/api/types.ts` | M/L — 1 week |
| P1 | Auto-switch pauses without telling the agent to commit, write a handoff note and push. The pause keeps an existing note but does not give the agent the step | Auto-switch (step 3); Handoff note | `core/src/api.rs` (`auto_switch_provider`, `pause_session`), `core/src/sessions.rs` input, the handoff skill or a new tool call | M — 3–5 days |
| P1 | Auto-switch does not act when the agent reports that it hit a limit. There is no tool or event for that report, so a provider with no usage number never triggers a switch | Where usage numbers come from (last line) | `core/src/mcp.rs` tool or event, `core/src/usage.rs`, `core/src/api.rs`, the Ask and notify or Handoff skill | M — 3–5 days |
| P1 | There is no setting for a DeepSeek minimum balance. The usage code keeps a minimum if a row already has one, but no UI or provider field sets it | Auto-switch rules; Settings / Providers | `app/src/components/ProviderDialog.tsx`, `app/src/components/SettingsDialog.tsx`, `core/src/providers.rs`, `core/src/usage.rs`, `core/src/api.rs`, `app/src/api/types.ts`, catalogs | S — 1–2 days |
| P1 | Closing the window is a quit request; there is no close-to-background path that keeps the core and the phone connection alive | Desktop app architecture (window close) | `app/src/App.tsx` window close handling, `src-tauri/src/main.rs` quit and core lifecycle, window hide/reopen | M — 3–5 days |
| P2 | A session cannot be dragged from one project to another. There are no drag handlers or drop targets, and no route moves a session between projects | Projects | `app/src/components/Sidebar.tsx`, `app/src/App.tsx`, `core/src/api.rs` move route, `app/src/api/types.ts`, `app/src/state/tabs.ts` | M — 3–5 days |
| P2 | No Tailscale Funnel action. The app reads tailnet addresses but never offers to make a demo public when asked | Demo servers / Optional Tailscale | `core/src/tailscale.rs`, `core/src/demo.rs`, `core/src/api.rs`, `app/src/components/SessionSidePanel.tsx`, types and catalogs | S — 2–4 days |
| P2 | An installed app-owned client copy cannot be updated from the app. The Install action appears only when the client is missing, so an older copy under the data folder has no update path | Installing a client | `app/src/components/AiConfigDialog.tsx`, `core/src/clients.rs` install state and version check, `core/src/api.rs` install route | S — 1–2 days |
| P2 | The launch form has no recent-session repository list and no GitHub search. The repository is the project picker; GitHub lives only in Add project | Launching a session / Sources | `app/src/components/NewSessionDialog.tsx`, reuse the GitHub picker from `AddProjectDialog.tsx`, `app/src/api/client.ts`, types | M — 3–5 days |
| P2 | The launch form puts the first prompt outside Additional settings. The uncommitted-changes choice and environment overrides are inside, but the first prompt is not | Config overrides (launch form grouping) | `app/src/components/NewSessionDialog.tsx`, the 12 catalogs | S — 1 day |

## Detail and evidence

### P0

**1. Desktop pairing screen.**
The core has the pairing state machine, the QR PNG route and the local pairing and device routes in
`core/src/link/`. The desktop Settings Devices page loads `/api/devices`, lists phones with platform,
last-seen time and connection state, and removes one. It has no call to `/api/pairing`, no QR image,
no six-digit code, and no Approve or Cancel action. The empty-state hint tells the user to scan a
code that the desktop never shows. Without this screen the release pairing flow cannot start; only a
debug pasted ticket can.

**2. Phone UI package.**
The phone layout exists at `app/phone.html` and `app/src/phone/`, but it is not packaged or served
to the shell. `app/vite.phone.config.ts`, `scripts/package-phone-ui.mjs` and
`app/src/api/transport.ts` do not exist. The link method list includes `ui.manifest` and
`ui.download`, but `core/src/link/app.rs` returns unsupported for both. The phone shell has its own
hand-written screens, not the computer's UI package. The spec's thin-shell model, package version
comparison, stored package, custom protocol and update-on-connect behavior are therefore absent.
The message view itself is excluded from this audit; this gap is the package build and delivery that
the shell needs to load the existing phone layout.

**3. Android and iOS projects.**
`phone-shell/` has the Tauri crate, the platform keychain stores, the link client, the forwarding
code and the shell UI. It has no generated Android or iOS project. The CI checks the phone shell
catalogs, types, UI build and Rust tests, but there is no Android SDK/NDK job and no APK artifact.
The spec's Android app, signed APK, self-updater and Play build cannot ship.

### P1

**4. Phone upload, file and image reads, and diffs.**
The link allowlist includes `sessions.upload`, `files.image` and `diff.read`, but `route_for` returns
unsupported for `sessions.upload` and `files.image`. The phone shell and the phone layout have no
upload button or file picker, no file and image view, and no diff view. `files.list`, `files.read`
and `diff.read` exist for the desktop. The spec's phone upload and its files, images and diffs view
are missing.

**5. Phone notifications.**
`phone-shell/src-tauri/Cargo.toml` has the barcode scanner and opener plugins, not a notification
plugin. The shell has no notification strings or code: no permission request, no notification for a
waiting session or a question, no missed list on open, and no click handler that opens the session.
The desktop notification path is built; the phone path is not.

**6. Browser profile encryption.**
The session profile folder is private (`0700`), and the profile keeps logins across close, reopen,
quit and crash. The profile content is not encrypted. `.dev/browser.md` lists "Encrypting the
profile at rest" as not built, and the spec's Browser tools section says the same.

**7. Browser console and network log tools.**
The agent tools are open, read, click, type, wait and screenshot. There are no console or network
log tools. The spec names both, and `.dev/browser.md` lists them as not built.

**8. Browser live-view click and type.**
The Browser tab shows the latest screenshot and the tab controls. It has no click or typing overlay,
and the core has no route that forwards a live-view pointer or key event to the page. The agent's
click and type tools work; the user's live-view input does not. The spec and `.dev/browser.md` mark
this as not built.

**9. Auto-switch pause instruction.**
`auto_switch_provider` selects a target; when none has room it calls `pause_session`.
`pause_session` stops the browser, closes demo tunnels, stops the process and sets the paused state.
It never writes to the agent's input, so the agent is not told to commit, write a handoff note and
push before the pause. The handoff note tool and the close commit path exist, but the spec's
out-of-usage step is not performed by the IDE.

**10. Agent-reported limit.**
The official usage sources update a provider's rows. When a provider has no number, `usage_state`
returns `Unknown` and auto-switch does nothing. There is no tool or event through which an agent
reports that it hit a limit, and no path that turns that report into a switch or pause. The spec's
last rule under "Where usage numbers come from" is not implemented.

**11. DeepSeek minimum balance.**
The provider and config forms have name, sign-in, server, model, effort, arguments and overrides.
They have no minimum-balance field. `core/src/usage.rs` has a function that keeps a minimum already
present on a row, but the UI and provider model never set it. The spec says each DeepSeek provider
has a minimum balance that I set.

**12. Window close.**
The Tauri window close event is prevented, and the app treats it as a quit request. With sessions
running, the Quit dialog appears. Confirm calls `quit_app`, which exits the app and stops the core.
Cancel leaves the window open. There is no action that closes or hides the window and keeps the core
and the phone connection running. The spec says closing the window keeps the sessions and the phone
connection, and the window reconnects when opened again.

### P2

**13. Drag a session between projects.**
The sidebar rows have no `draggable` attribute, drag start, drop target or drag-over handler. The
API has no route that changes a session's project. A session therefore stays in the project it was
created in. The spec lists the drag as a project action.

**14. Tailscale Funnel.**
`core/src/tailscale.rs` reads the CLI status and the machine name and builds a tailnet address. It
has no Funnel command, route or UI action. The spec says the IDE makes a demo public with Funnel
only when I ask.

**15. Updating an installed client copy.**
The AI config form shows the Install action only when the client is not installed. The client
lookup can use the app's own copy under the data folder, but the form has no Update or Reinstall
action for an existing app copy, so an older copy cannot be replaced from the app. The spec says a
successful install replaces an older app copy.

**16. Launch form repository picker.**
The launch form's repository is the project combobox. Its branch combobox lists local and remote
refs. It has no recent-session repository list and no GitHub search. GitHub sign-in, repository
listing and clone live in Add project. The spec says the launch form can pick a repository from
recent sessions or search GitHub after connecting.

**17. Launch form grouping.**
The first prompt is a top-level field. The uncommitted-changes choice and the environment override
box are inside Additional settings. The spec says the first prompt, the uncommitted choice and the
overrides are grouped under Additional settings.

## Not counted

- The phone shell has not run on a real device or emulator. The browser handoff and background
  forwarding are wired but need a device check.
- The login-item and keep-awake behavior is registered by the desktop shell but has not been checked
  in a packaged, signed build.
- A real in-app update needs the update signing key, the public key and a published release. The
  code and release workflow exist.
- The phone shell message view and the Chromium download are in flight and excluded by request.
- Client discovery and installation on Windows and Linux, and the iPhone app, are later work.

## Method

Read the spec, phone shell, browser and provider/config documents, then checked the desktop UI,
core, link crate, phone shell, Tauri shell, tests and release workflows at `fba755c`. I changed only
this report.
