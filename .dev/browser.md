# Built-in browser

This document describes how the session browser works. Behavior comes from `.dev/SPEC.md`; this document names the parts and the reasons.

## Shape

The core service runs one Chromium process per session. It drives the process over the DevTools protocol through the `chromiumoxide` crate. The desktop UI shows a live screenshot, not an embedded web view.

The Tauri shell could host a web view per session with a separate WebKit data store. It does not, because a web view cannot give the core:

- a page screenshot for the share tool;
- a stable way for agent tools to click, type, read and wait;
- the same behavior in the browser-based Playwright suite.

The core-owned Chromium is also what `.dev/SPEC.md` names. The shell stays a thin window.

## Isolation and lifetime

- Each session has a profile folder at `<data dir>/browsers/<session id>` with private permissions (0700).
- The profile holds the session's cookies, storage and logins. Two sessions never share a profile, so a login never crosses sessions.
- The process starts on the first browser action, not when the session starts. Opening the Browser tab with no address starts nothing.
- Close and pause stop the process and clear the tabs. The profile stays, so logins survive a close, a reopen, a crash and a quit.
- Delete and shutdown stop the process as well. The profile is not removed yet.
- The profile is not encrypted at rest yet.

## Core routes

All routes take the session id in the path. A closed session refuses every action with 409. An unknown session returns 404.

| Route | Job |
| --- | --- |
| `GET /api/sessions/{id}/browser` | Current tabs, active tab, loading and screenshot sequence |
| `POST /api/sessions/{id}/browser/start` | Start the browser without opening a page |
| `POST /api/sessions/{id}/browser/stop` | Stop the browser and close its tabs |
| `POST /api/sessions/{id}/browser/open` | Open a URL in the active tab, or a new tab |
| `POST /api/sessions/{id}/browser/back` | Go back in the active tab |
| `POST /api/sessions/{id}/browser/forward` | Go forward in the active tab |
| `POST /api/sessions/{id}/browser/reload` | Reload the active tab |
| `POST /api/sessions/{id}/browser/tabs` | Add a tab, optionally at a URL |
| `POST /api/sessions/{id}/browser/tabs/{tab}/activate` | Make a tab active |
| `POST /api/sessions/{id}/browser/tabs/{tab}/close` | Close a tab |
| `GET /api/sessions/{id}/browser/screenshot?tab=` | Latest PNG for a tab |

Only `http` and `https` addresses are accepted. A full address is required. Each session context has at most 12 tabs. The tab id includes the session, so a tab id from one session is unknown in another.

## Agent tools

The MCP server exposes six browser tools. Each tool uses the calling session's own context; no tool takes a session id. A closed session gets an error and no page.

| Tool | Arguments | Result |
| --- | --- | --- |
| `browser_open` | `url` | Opens the page and returns the title and visible text |
| `browser_read` | none | Visible text of the active page, up to 20,000 characters |
| `browser_click` | `selector` | Clicks a CSS selector and returns the page state |
| `browser_type` | `selector`, `text` | Focuses the element and types |
| `browser_wait` | `selector?`, `text?`, `timeoutSeconds?` | Returns when the condition holds or the timeout ends (1–15 seconds) |
| `browser_screenshot` | `caption?` | Captures a PNG, stores it in the shared file store and tells the UI |

The Browser checks skill tells agents when to use them.

## UI

The Browser tab is the third tab of the session side panel.

- One tab strip lists the session's tabs and adds a tab.
- One address field opens a page; Enter or the Open button submits.
- Back, Forward and Reload act on the active tab and follow its history state.
- A loading line shows while a request runs or the active tab is loading.
- The live view is the latest captured screenshot. It refreshes when the screenshot sequence changes. Polling reads the state once a second while the panel is mounted.
- A tab, close button and address field carry labels for keyboard and screen-reader use. All text comes from the 12 language catalogs.

## Tests

- `core/tests/api.rs` covers tool scoping between two sessions, a tab id from another session, a closed-session refusal, a non-http address, and the screenshot share. The core test state uses a stub engine that fetches a local page over HTTP and returns a small PNG.
- `app/src/components/BrowserPanel.test.tsx` covers opening an address, the loading state, Back, Reload, tab switching, adding and closing a tab, per-session state and an error.
- `e2e/23-browser.spec.ts` drives two sessions against a local page with real Chromium. It checks separate tabs, a live image, and the type, click, wait and read tools. It uses the Playwright Chromium binary through the `CHROME` environment variable.

## Not built yet

- Downloading Chromium on first use.
- Encrypting the profile at rest.
- Clicking and typing directly in the live view.
- Console and network log tools.
- Removing the profile when a session is deleted.
