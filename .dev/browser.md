# Built-in browser

This document describes how the session browser works. Behavior comes from `.dev/SPEC.md`; this document names the parts and the reasons.

## Shape

The core service runs one Chromium process per session. It drives the process over the DevTools protocol through the `chromiumoxide` crate. The desktop UI shows a live screenshot, not an embedded web view.

The Tauri shell could host a web view per session with a separate WebKit data store. It does not, because a web view cannot give the core:

- a page screenshot for the share tool;
- a stable way for agent tools to click, type, read and wait;
- the same behavior in the browser-based Playwright suite.

The core-owned Chromium is also what `.dev/SPEC.md` names. The shell stays a thin window.

## Engine

The core runs one browser binary for all sessions. Each session still gets its own process and profile.

- An installed Chrome or Chromium is used first. The `CHROME` environment variable points at a specific binary.
- When none exists, the core downloads the pinned Chrome for Testing build for the computer's architecture from `https://storage.googleapis.com/chrome-for-testing-public`. The version and the SHA-256 digest for each supported macOS architecture are constants in `core/src/browser/install.rs`.
- The archive is saved below `<data dir>/chromium` with private permissions while its SHA-256 and the storage server's published MD5 (`x-goog-hash`) are computed. A mismatch removes the download and stops the install.
- Unpacking rejects absolute paths, parent paths, drive letters, backslashes and links that leave the folder. It caps the entry count and the uncompressed size. Links are created only after every regular file, and only when their target stays inside the folder.
- The unpacked build is renamed into `<data dir>/chromium/<version>`, the program is marked executable, and a `<data dir>/chromium/version` marker records the build. A later use reuses it. Deleting the folder makes the next use download again.
- A failure removes the staging folder, so no partial browser stays. The session keeps its terminal. The Browser panel shows the phase and progress, and Retry asks the core to start again.
- The download needs the network. Tests use a fake server and a fake archive, so the real pinned builds are downloaded only when the code runs on a computer without Chrome.

## Isolation and lifetime

- Each session has a profile folder at `<data dir>/browsers/<session id>` with private permissions (0700).
- The profile holds the session's cookies, storage and logins. Two sessions never share a profile, so a login never crosses sessions.
- The process starts on the first browser action, not when the session starts. Opening the Browser tab with no address starts nothing.
- Close and pause stop the process and clear the tabs. The profile stays, so logins survive a close, a reopen, a crash and a quit.
- Delete and shutdown stop the process as well. The profile is not removed yet.
- The profile is not encrypted at rest. Private permissions are the only protection today. A real fix encrypts the whole profile, because cookies, local storage, the cache and Chromium's own password file all hold login state. The documented platform options do not fit: APFS encryption needs a separate encrypted volume or disk image per session, and mounting one around each browser start adds stale-mount and crash recovery that the app does not have. Chromium's own `--password-store=basic` setting is not encryption. Until that work is designed and tested, the app does not claim encrypted storage.

## Core routes

All routes take the session id in the path. A closed session refuses every action with 409. An unknown session returns 404.

| Route | Job |
| --- | --- |
| `GET /api/sessions/{id}/browser` | Current tabs, active tab, loading and screenshot sequence |
| `POST /api/sessions/{id}/browser/start` | Start the browser without opening a page |
| `POST /api/sessions/{id}/browser/stop` | Stop the browser and close its tabs |
| `POST /api/sessions/{id}/browser/setup` | Start or retry fetching the browser binary |
| `POST /api/sessions/{id}/browser/open` | Open a URL in the active tab, or a new tab |
| `POST /api/sessions/{id}/browser/back` | Go back in the active tab |
| `POST /api/sessions/{id}/browser/forward` | Go forward in the active tab |
| `POST /api/sessions/{id}/browser/reload` | Reload the active tab |
| `POST /api/sessions/{id}/browser/input/click` | Click the active page at a fraction of its viewport |
| `POST /api/sessions/{id}/browser/input/type` | Insert text at the focused element |
| `POST /api/sessions/{id}/browser/input/key` | Press one named key |
| `POST /api/sessions/{id}/browser/tabs` | Add a tab, optionally at a URL |
| `POST /api/sessions/{id}/browser/tabs/{tab}/activate` | Make a tab active |
| `POST /api/sessions/{id}/browser/tabs/{tab}/close` | Close a tab |
| `GET /api/sessions/{id}/browser/screenshot?tab=` | Latest PNG for a tab |

Only `http` and `https` addresses are accepted. A full address is required. Each session context has at most 12 tabs. The tab id includes the session, so a tab id from one session is unknown in another.

## Live input

The live view is a screenshot inside a focusable layer. The layer covers the image, so a pointer position has the same fraction on the image and on the page viewport. The panel sends that fraction, not a pixel position. The core reads the page's own viewport size and turns the fraction into CSS pixels, which keeps clicks correct when the image is scaled and when the display density differs.

- A click sends a mouse move, press and release to the active tab. The page receives the click at the point the user chose.
- A printed character collects with the characters typed just after it and travels as one insert. This keeps the order and lowers the number of requests. A named key flushes the pending text first, then presses.
- Named keys are Enter, Tab, Escape, Backspace, Delete, the arrows, Home, End, PageUp and PageDown. Shift and every other modifier work with them.
- Letters and digits travel as key events when Control, Meta or Alt is held. One modifier mask goes with the key: Alt 1, Control 2, Meta 4, Shift 8. A command modifier removes the inserted text, so the page sees a shortcut. Control or Command with A, C, V, F and the other letter chords therefore reach the page.
- On a Mac, the core adds the matching editor command to the DevTools event, because Chromium does not run the command from the key event alone. Select all, copy, cut, paste, undo, redo and the word and line movements then act on the page. Command with F reaches a page that handles it, but the headless browser has no find bar to show in the screenshot.
- The panel keeps the shortcuts of the app window: Command with Q, W, M or H on a Mac, and Control with W elsewhere. It does not cancel them and does not send them, so the app and the system still act on them. A key the core does not define is left with the app as well.
- The live layer keeps focus and its application role. Tab and Escape still travel as named keys, so the page keeps its own focus changes and its own cancel key. A reserved window shortcut is not cancelled, so the app and the system still act on it.
- All input for one panel runs in order through one queue, so a click cannot arrive before the typing that came before it.
- The routes take the session id, and the manager uses that session's own context. A closed session is refused before the manager runs. A point outside the page and an unknown key return 400.

## Agent tools

The MCP server exposes eight browser tools. Each tool uses the calling session's own context; no tool takes a session id. A closed session gets an error and no page.

| Tool | Arguments | Result |
| --- | --- | --- |
| `browser_open` | `url` | Opens the page and returns the title and visible text |
| `browser_read` | none | Visible text of the active page, up to 20,000 characters |
| `browser_click` | `selector` | Clicks a CSS selector and returns the page state |
| `browser_type` | `selector`, `text` | Focuses the element and types |
| `browser_wait` | `selector?`, `text?`, `timeoutSeconds?` | Returns when the condition holds or the timeout ends (1–15 seconds) |
| `browser_screenshot` | `caption?` | Captures a PNG, stores it in the shared file store and tells the UI |
| `browser_console` | `limit?` (1–200) | The last console messages, oldest first |
| `browser_network` | `limit?` (1–200) | The last network requests, oldest first |

The Browser checks skill tells agents when to use them.

The console and network records live in memory with the session browser. The core starts the Runtime, Log and Network protocol domains on each page and registers the listeners once per browser process. A listener stores one line per console call, page error, log entry, request, response and failure. The buffer keeps the last 200 of each kind and drops the oldest. Text is cleaned to one line and capped at 1000 characters. An address loses its login details and the values of sensitive query parameters such as a token, a key or a password. Before a tool returns text, the core removes every stored provider key, as it does for session output. The records are cleared when the browser stops. The tools do not start a browser: with no page they return the no-page error.

## UI

The Browser tab is the third tab of the session side panel.

- One tab strip lists the session's tabs and adds a tab.
- One address field opens a page; Enter or the Open button submits.
- Back, Forward and Reload act on the active tab and follow its history state.
- A loading line shows while a request runs or the active tab is loading.
- While the browser binary is prepared, the panel shows the step and the progress. A failure shows the reason and a Retry action.
- The live view is the latest captured screenshot. It refreshes when the screenshot sequence changes. Polling reads the state once a second while the panel is mounted.
- The screenshot has a focusable layer over it. A click on the layer becomes a click on the page at the same relative point. Typing goes to the focused page element. A short line under the image says that the view is live and takes clicks and keys.
- The layer has an application role and a label. A tab, close button and address field carry labels for keyboard and screen-reader use. All text comes from the 12 language catalogs.
- The console and network records have no separate panel view. The spec gives agents the tools and does not name a user view.

## Tests

- `core/src/browser/logs.rs` tests the length caps, the oldest-entry drop, the address redaction, the status and failure lines, and the empty messages.
- `core/src/browser/chromium.rs` tests the key definitions: letters and digits and their codes, the Shift value, every command modifier that removes the text, and the keys that are refused.
- `core/tests/api.rs` covers tool scoping between two sessions, a tab id from another session, a closed-session refusal, a non-http address, and the screenshot share. It also covers live click, type and key input staying inside one session, each modifier bit with a letter, a digit with two modifiers, a click point outside the page, an unsupported key, the closed-session refusal for the input routes and both log tools, the console and network results, the 200-entry bound, sensitive-address redaction, and a stored provider key removed from a network address. The core test state uses a stub engine that fetches a local page over HTTP, returns a small PNG and records its own console and network lines. A separate test serves a fake browser archive over loopback, checks that a wrong checksum fails and leaves no browser, checks that the session still exists, and checks that Retry installs a verified archive and marks the program executable.
- `app/src/components/BrowserPanel.test.tsx` covers opening an address, the loading state, Back, Reload, tab switching, adding and closing a tab, per-session state, an error, the download progress, the failed-install Retry, a live click mapped to a page point, live typing and named keys, the modifier mask of letter and digit chords, named keys with Control and Alt, and the window shortcuts that stay with the app on a Mac and on other platforms.
- `e2e/23-browser.spec.ts` drives two sessions against a local page with real Chromium. It checks separate tabs, a live image, the type, click, wait and read tools, and that the console and network tools return a page message and the page request with its status. A second test clicks and types through the live view and checks the page changed. A third test sends the platform's select-all and copy chord through the live view and checks the page's `select` and `copy` events. It uses the Playwright Chromium binary through the `CHROME` environment variable. The system clipboard, a paste from another app and a page that draws its own text need a real site and a person, because the headless browser and the native menu differ there.

## Not built yet

- Encrypting the profile at rest. The reasons and the missing design are in Isolation and lifetime.
- Removing the profile when a session is deleted.
- A user view of the console and network records. The spec names the agent tools only.
