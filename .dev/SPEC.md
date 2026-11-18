# elesdee — Spec

Last updated: 18 November 2026

## Purpose

Build an IDE that runs Claude Code and Codex sessions the way I want, with less manual work. It replaces onoRCA.dev, which does not give enough control and does not save enough time.

The app is public. Anyone can install it from this repository's GitHub releases.

This spec is a first draft. More requirements will follow.

## Desktop app architecture

The desktop app is a Tauri app for macOS. Its core is a separate Rust process, the core service. The app starts and stops it. It is not installed as a macOS system service. The window only shows what the core service does.

| Part | Job |
| --- | --- |
| Core service (Rust) | Runs agent sessions in terminals, manages Git branches and worktrees, tracks usage, saves session state, holds the iroh connection to the phone |
| Desktop window (Tauri) | Shows sessions, usage and projects. Sends my input to the core service. |
| Mobile app (React Native) | Same view and input as the window, over iroh |

- If the window crashes or I close it, sessions keep running and the phone stays connected. When the window opens again, it reconnects.
- When I quit the app while sessions are running, the app asks me first. If I confirm, the core service saves every session and stops. On the next launch, the sessions resume. Crash recovery (below) covers a crash of the core service or a restart of the Mac.
- Why Tauri and not Electron: iroh is written in Rust, so the service uses it directly. Tauri also uses less memory, which matters for an app that runs all day. On macOS, Tauri's built-in web view works well.

## Window layout and sessions

Sessions are the main view. A file viewer and code editor is an extra panel that I open only when I need it. The layout will be settled through several rounds of UI mockups.

**Session types**

- Agent session: runs Claude Code or Codex, with a preset, provider, branch and worktree.
- Terminal session: a plain terminal, with no agent.

**Sessions sidebar**

- Shows all sessions in a vertical list, in one of two views:
  - Grouped: sessions sit under their project. I can collapse a project. A collapsed project shows a pulsing dot when a session under it is running.
  - Flat: no project groups. Each session shows its project name in small text under the session name.
- A filter shows only running sessions. In the grouped view, the filter expands every project that has a running session.

**Session menu**

- Right-click a session in the sidebar, or its tab, to open a menu of actions.
- One action is Close. Close stops the session. The app asks me to confirm first.
- Close is the only way to stop a session.

**Closed sessions**

- A closed session disappears from the sidebar.
- A filter at the top of the sidebar, "Show closed sessions", brings closed sessions back into the list. They look different from open sessions, for example greyed out.
- I can reopen a closed session. The app opens it in a new tab, loads its full log, and restarts the agent with the provider it last used. The agent gets the full log as context.
- Close keeps the session's branch but deletes its worktree folder, to save disk space. Before it deletes the folder, the app commits any uncommitted changes to the session branch, so no work is lost.
- If the session branch has work that is not pushed to the remote, the Close confirmation warns me. The app does not push for me.
- Reopen re-creates the worktree from the session branch.

**Tabs**

- Click a session in the sidebar to open it as a tab.
- A tab cannot be closed, only minimized. Minimize takes the tab out of the tab bar. The session keeps running and stays in the sidebar.

**File viewer and code editor (side panel)**

- Closed by default. I open it when I want to look at or edit files.
- Shows the file tree of the current session's worktree or folder.
- Code editor with full syntax highlighting. I can edit and save files.
- Not a main feature: it supports quick edits, not full IDE work.

## Agent support and presets

Version 1 supports two coding agents: Claude Code and Codex. Other agents can come later.

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

## Built-in tools and skills for sessions

Every agent session gets the same app tools and skills, whatever the agent or provider. They come from elesdee itself, not from a provider's add-on such as a browser extension.

**How sessions get them**

- The core service runs one MCP server, "elesdee". Claude Code and Codex both support MCP servers ([Codex docs](https://developers.openai.com/codex/mcp)).
- At launch, the app adds this MCP server and the skills to the agent through the launch overrides. It does not change global config or write into the repo.
- Skills use the SKILL.md format, which both Claude Code and Codex read ([Codex docs](https://developers.openai.com/codex/skills.md)).
- Agents read only a skill's name and description at start, and load the rest when they decide they need it. So a short note in each agent's starting instructions also tells it that it runs inside elesdee and points it to the elesdee skill.

**Browser tools**

- The core service runs its own browser (Chromium), separate from my personal browser. No browser extension is needed.
- Chromium is not bundled in the app. The app downloads it the first time a session needs a browser. This keeps the app download and its updates small.
- Each session gets its own browser context: its own tabs, cookies and storage. The session keeps its logins when I quit and relaunch the app, after a crash or a restart of the Mac, and when I close and reopen the session. Login data is stored encrypted on my Mac.
- Tools: open a page, click, type, read the page, take screenshots, read console and network logs.
- I can watch a session's browser from the desktop window and from the phone.
- Starting point: Microsoft's Playwright MCP server, which already has 70+ browser tools ([Playwright](https://playwright.dev/mcp/introduction)).

**Session tools**

| Tool | What it does |
| --- | --- |
| List sessions | Lists sessions with their project, status and branch |
| Start session | Starts a new session with a preset, project, source branch and first prompt. I must approve each new session first, on the desktop or the phone. The sidebar marks which session started it. |
| Send message | Sends input to another session |
| Read session | Reads another session's recent output and status: running, waiting for input, or finished |
| Wait for session | Waits until another session finishes or needs input |
| Register demo server | Tells the app that a demo server runs on a port. Returns an address I can open remotely (see Tailscale and demo servers). |
| Notify me | Sends me a notification on the desktop and the phone |
| Ask me | Asks me a question and waits for my answer, on the desktop or the phone |
| Share file | Adds a file or screenshot to the session's view, so I can see it on any device |
| Check usage | Shows the session's remaining context and the provider's remaining usage |

- An agent cannot close a session. Only I can, through the session menu.

**Skills given to every session**

| Skill | What it teaches |
| --- | --- |
| elesdee | What elesdee is. The session has its own branch and worktree. How to use the session tools to start and coordinate other sessions. |
| Demo servers | Start a demo server, register its port, and give me the address that works on my phone |
| Handoff | The standard handoff note, and when to commit, write it and push: provider switch, out of usage, Close |
| Browser checks | Check UI changes in the built-in browser, take screenshots and share them with me |
| Ask and notify | Ask me through the app when blocked, instead of waiting. Tell me when work is done. |
| Coordinate sessions | Split a large task across sessions, write clear briefs, track progress, and merge their branches |

## Crash recovery

After a crash or restart, the IDE reopens every session that was running and resumes each one where it stopped.

- The IDE saves the state of each session while it runs: agent, preset, provider, folder, branch, and the agent's own session ID.
- On restart, the IDE relaunches each session with the same settings and resumes the agent's conversation.
- I do not need to do anything to restore my work.

## Providers, usage and auto-switch

I can switch provider inside a running session with one button, and the IDE can switch for me when usage runs low.

**Accounts**

- I can add more than one account per provider, for example several Claude subscriptions.
- Each account is a provider choice in a session.

**Manual switch**

- One button switches the current session to another provider or account.
- The work and context of the session carry over to the new provider. Inside Claude Code (for example Anthropic to DeepSeek), the same conversation continues. Between Claude Code and Codex, the new agent gets a handoff summary of the work so far.

**Usage panel (sidebar or tab)**

Shows every account at the same time:

| Account type | What the panel shows |
| --- | --- |
| Claude Code (each subscription) | Weekly usage remaining; context remaining in the current session |
| Codex | Weekly usage remaining; context remaining in the current session |
| DeepSeek (through Claude Code) | Account balance from the DeepSeek API, so I know when to top up; context remaining in the current session |

**Allowed providers per session**

- For each session, I choose which providers and accounts it may use.
- Auto-switch uses only providers from this list.

**Auto-switch**

1. The IDE watches the usage of the current provider.
2. When usage nears its limit, the IDE switches the session to an allowed provider that has usage left.
3. If no allowed provider has usage left, the IDE tells the agent to commit all its work to the session branch, write a handoff note, and push the branch to the remote.
4. The IDE then stops the session.
5. Later, I can resume the session from that saved state with little effort.

## Projects

A project is a label that groups sessions.

- Creating a project is fast: one action and a name.
- I can drag a session from one project to another.

## Launching a session

When I launch a session, I choose where the code comes from, and the IDE gets the latest code first.

**Sources**

- A local Git branch
- A remote GitHub branch
- A plain folder (no Git)

**Auto-sync at launch**

- From a remote branch: the IDE gets the latest version of that branch.
- From a local branch that tracks a remote branch (for example `main`): the IDE pulls the latest changes before the agent starts.

**Branch and worktree per session**

- Every new session gets its own Git branch when it is created.
- Every session gets its own Git worktree, so sessions never change each other's files.
- Before a session stops for good, the agent is told to push its branch to the remote.

## Tailscale and demo servers

When I work remotely, every demo server a session starts must have an address my remote device can open. The IDE finds or makes that address.

1. The IDE checks if Tailscale runs. `tailscale status --json` returns a `BackendState` field, which is `Running` when Tailscale is up ([check](https://alexwlchan.net/til/2025/check-tailscale-is-running/)). The same output gives the machine's Tailscale name ([forum](https://forum.tailscale.com/t/find-out-tailnet-name-from-script/5271)).
2. If Tailscale runs, the IDE runs `tailscale serve <port>` for each demo server. This gives an HTTPS address that only devices in my Tailscale network can open ([Tailscale docs](https://tailscale.com/docs/use-cases/application-testing/share-local-dev-server-with-team)).
3. If Tailscale is not installed or not running, the IDE suggests that I install or start it. Until then, it uses a fallback (open question below).
4. The IDE tells the agent which address to use, so links the agent prints work on my remote device.

To share a demo with people outside my Tailscale network, Tailscale Funnel can make it public ([Tailscale docs](https://tailscale.com/docs/use-cases/application-testing/share-local-dev-server-with-internet)). The IDE does this only when I ask.

## Mobile companion app

The mobile app is the only way to control the IDE remotely. It is built with React Native, for iOS and Android. The Android app ships first (see Releases and updates). There is no web interface. The app talks to the desktop app in real time.

From the mobile app I can:

- See all running sessions, updated in real time.
- Type and send input to any session.
- Upload files into a session.
- View what a session creates: files, images, diffs and running demo servers.

**Pairing (one time)**

1. The desktop app shows a QR code.
2. The phone scans it. The QR code holds the desktop app's identity key, the relay server address and a one-time pairing secret.
3. The two devices save each other's keys. From then on, each knows the other is genuine.

**After pairing**

- The phone connects to the desktop app from any network, with no more setup.
- The desktop app lists paired devices. I can remove a device at any time.

## Connection technology

Recommendation: iroh. Each device is reached by its public key. iroh tries a direct link first and falls back to a relay when the network blocks it ([iroh docs](https://iroh.computer/docs/overview)). Fallback plan: WebRTC data channels.

**How it works**

1. At pairing, the phone gets the desktop app's public key from the QR code.
2. The phone connects to that key. The relay server helps the two devices find each other.
3. iroh then tries a direct link. If the network blocks it (common on mobile data), traffic stays on the relay.
4. All traffic is encrypted end to end. The relay cannot read it ([iroh docs](https://docs.iroh.computer/languages/wasm-browser)).

**Relay server**

- We run our own relay for all users. iroh accepts a custom list of relay servers instead of its default ones.
- The relay does the jobs that WebRTC splits across three services: introduction, address discovery and backup relay.

**Mobile app (React Native)**

- I found no React Native library for iroh. Its official mobile libraries are for Swift and Kotlin.
- Plan: write one small Rust library that holds iroh and our message format. Turn it into a React Native module for iOS and Android with uniffi-bindgen-react-native, a tool from Mozilla ([Mozilla](https://hacks.mozilla.org/2024/12/introducing-uniffi-for-react-native-rust-powered-turbo-modules/)). The iroh team names this route as one that works ([iroh blog](https://iroh.computer/blog/ffi-updates)).
- The desktop service uses the same Rust library, so pairing, messages and reconnection are written once.
- The test build must run this module on a real iPhone and a real Android phone.

**iroh compared with WebRTC**

| Question | iroh (recommended) | WebRTC data channels (fallback) |
| --- | --- | --- |
| Servers to run | One relay server | Rendezvous server, STUN, TURN relay |
| Pairing | QR code holds one public key | QR code holds keys and server details; an offer/answer exchange on each connect |
| Mobile libraries | Official Swift and Kotlin ([iroh docs](https://docs.iroh.computer/languages)) | Mature native WebRTC libraries |
| Maturity | Version 1.0 since June 2026 ([Pinggy](https://pinggy.io/blog/iroh_1_0_dial_keys_not_ips/)) | Used in production for many years |

**Risks**

- iroh is new, so there is less real-world use behind it.
- The iroh team paused its mobile libraries in 2025 ([iroh blog](https://iroh.computer/blog/ffi-updates)). They are official again now, but we must test them on real phones.

**First step: a test build**

Pair a phone and the desktop app by QR code and stream a live session over iroh. Test on Wi-Fi and on mobile data. If it works well, commit to iroh. If not, switch to WebRTC.

**Design rules**

- Keep the app's messages separate from the transport, so a switch from iroh to WebRTC does not change the app.
- Both sides prove their identity with the keys saved at QR pairing.

## Releases and updates

The desktop app and the mobile apps are published through GitHub Releases in this repository. The repository is public, so apps can download releases without a GitHub login. An update installs over the existing app and keeps all its data: sessions, presets, accounts, paired devices and browser logins.

**Release builds**

- A GitHub Actions workflow builds and publishes each release. The Git tag of the release sets the version number.
- The signing keys are stored as GitHub Actions secrets, never in the repository.
- The core service is bundled inside the desktop app as a sidecar binary ([Tauri docs](https://v2.tauri.app/develop/sidecar/)). One update replaces the window and the core service together, so the two always have the same version.

**Desktop app (macOS)**

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

- The app updates itself. It checks GitHub for a new release, downloads the APK, and asks Android to install it over the old app. Android asks the user to confirm.
- To install an APK, an app that targets API level 26 or newer must hold the `REQUEST_INSTALL_PACKAGES` permission ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/12085295)).
- Users who prefer it can use Obtainium, an Android app that installs and updates apps from their GitHub release pages ([Obtainium](https://github.com/ImranR98/Obtainium)).

*Google Play phase*

- An app distributed on Google Play may not update itself by any method other than Google Play's update mechanism ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/9888379)). Self-update is also not on Play's list of allowed uses for the install permission ([Google Play policy](https://support.google.com/googleplay/android-developer/answer/12085295)).
- So the build for Google Play leaves out the self-updater and the install permission. The GitHub build keeps them.
- Play App Signing lets a developer upload an existing app signing key, with Google's PEPK tool ([Play Console Help](https://support.google.com/googleplay/android-developer/answer/9842756)). We upload the key that signs the GitHub builds, so Play updates install over GitHub installs.

*Developer verification (both phases)*

- Google now verifies the identity of Android developers. From 30 September 2026, the protections begin for users in Brazil, Indonesia, Singapore and Thailand who install apps from participating stores on certified devices. In 2027, Google plans to expand verification worldwide to all apps on certified devices ([Android docs](https://developer.android.com/developer-verification)).
- Users can still install apps from unverified developers through a one-time "advanced flow", which includes a one-day wait. A free "limited distribution" account lets a developer share an app with up to 20 devices, with no ID and no fee ([Android blog](https://android-developers.googleblog.com/2026/03/android-developer-verification.html)).

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

**Version numbers**

- The desktop app and the mobile apps share one version number. They are built and released together.
- A store review can make the mobile app reach users later than the desktop app. So for a short time, the phone and the desktop can run different versions.
- The message format between them carries a version number. When the two versions cannot work together, the older side shows "update needed".

## Open questions

**Scope**

- [ ] What exactly does onoRCA.dev fail to do? This sets the priorities.

**Provider switching**

- [ ] Auto-switch threshold: what percent of the limit? Weekly limit only, or also the 5-hour limit?
- [ ] Switch order: a fixed priority list per session, or the provider with the most usage left?
- [ ] DeepSeek has a balance, not a weekly limit. At what balance does auto-switch treat it as "near the limit"?

**Remote access and mobile**

- [ ] Build the iroh test first (phone and desktop, on Wi-Fi and mobile data) before committing to iroh?
- [ ] Demo servers without Tailscale: is it OK to show them through the paired connection inside the app, as a fallback?

**Git**

- [ ] Session branch names: a fixed pattern (for example `session/<project>/<short-name>`)?
- [ ] Should the IDE open a pull request when a session finishes?
- [ ] If the base branch has uncommitted changes at launch, should the IDE stash them, stop, or ask?

**Sessions and terminals**

- [ ] Terminal sessions: do they belong to a project and get their own branch and worktree, or do they just open in a folder I choose?
- [ ] After a crash, a terminal session cannot resume the programs that were running in it. Is it enough to reopen it in the same folder with its earlier output shown?

**Tools and skills**

- [ ] Browser tools: use Microsoft's Playwright MCP server as it is, or build browser tools into the elesdee server?

**Releases and updates**

- [ ] A beta channel (GitHub pre-releases) next to stable releases?
- [ ] Apple Silicon only, or Intel Macs too?
- [ ] Which route for the iPhone app, when it ships: TestFlight, ad hoc or App Store?
- [ ] Register as a verified Android developer before the first GitHub release? Without it, once Google expands verification worldwide in 2027, users on certified devices must go through the "advanced flow" with its one-day wait.
- [ ] The app is public, so strangers will use our relay server. Who pays for it, what limits apply, and can users run their own relay?
