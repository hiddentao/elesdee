# Fix verification: merge review 3

Range checked: `bf37f60..2d9ab01` on `hiddentao/round-2-mockups`. The four commits are:

- `9539a96` serve phone UI packages by computer and keep the open build
- `fd0cbc0` desktop focus, status and modal fixes
- `003bc43` phone sheet, focus and list fixes
- `2d9ab01` harden the Tailscale Funnel lifecycle

The worktree started from stale `main` and was reset to `origin/hiddentao/round-2-mockups` before the checks. No product code or end-to-end test was edited.

## Result

| Area | Result | Note |
| --- | --- | --- |
| Funnel lifecycle | Pass, with finding 1 | The enable, close, pause, stop and startup paths are correct. The delete path still drops a public record when the turn-off fails. |
| Phone package serving | Pass, with finding 2 | Computer-scoped path, two computers, two updates, incomplete builds and traversal are correct. A narrow concurrent-check race can prune an in-flight build. |
| Desktop fixes | Pass, with finding 3 | All claimed behavior is present and the end-to-end tests pass. The new busy-dialog test does not exercise the new code. |
| Phone fixes | Pass | All claimed behavior is present and the unit and end-to-end tests pass. |

## Findings

### 1. Medium: deleting a closed session drops the only record of a public port when the turn-off fails

- Where: `core/src/state.rs:2573` (delete calls the disable and ignores its result), `core/src/state.rs:2290-2316` (a refused command is logged and the address is kept), `core/src/api.rs:3877-3916` (the worktree and session files are removed, then the session is removed).
- Scenario: close a session while Tailscale is stopped or slow. The close path keeps the public address on the record, as intended. The session is now closed and its stopped server hides the address. The user deletes the closed session. Delete tries the turn-off again; if Tailscale still refuses, the session and its address are removed anyway. The daemon can keep the persisted 443 Funnel mapping with no record left, so a later process that binds the same loopback port is reachable from the internet, and no startup sweep can clear it.
- Why it matters: this contradicts the new spec sentence at `.dev/SPEC.md:571` ("The next app start also retries, and clears the address only after the client accepts the command") and `.dev/api.md:389,394` ("keep the address on the record for a later retry" and "stays on the record until a later retry succeeds"). It is the same exposure that finding 4 of the review raised, left open on one path.
- Suggested fix: make the disable report whether any address remains. In the delete handler, attempt the turn-off before the destructive worktree and file removal, and return 422 with a retryable error while an address remains. Alternatively keep a pending-off list in `state.json`, outside the session record, that startup retries until the client accepts the command.

### 2. Low: two concurrent package checks for one computer can prune an in-flight build

- Where: `phone-shell/src-tauri/src/ui.rs:300-302` (the fast `find_build` path calls `use_build` without the gate), `:305` (the gate starts only for the download), `:326-352` (`use_build` writes `current.json` and prunes), `:779-800` (`prune_builds` deletes every non-kept folder, including `.tmp-*`).
- Scenario: a connect and a background reconnect for the same computer run `ensure` together. One is inside `download_and_install` with an unpacked `.tmp-*` folder. The other finds its build already installed and calls `use_build` outside the gate. Its prune can delete the temporary folder, so the download fails. If the prune lands between `place_build` and the other `use_build`, it can delete the new build after `current.json` names it, and the next open reports no package until the following connect.
- Impact: a failed status or one empty package screen; the build on screen is kept, and the next connect repairs it. No unsafe path or exposure.
- Suggested fix: take the gate around the fast `find_build` path too (or a shared lock around `use_build`), or make `prune_builds` skip `.tmp-*` folders younger than the unpack timeout.

### 3. Low: the new busy-dialog focus test does not exercise the new code

- Where: `app/src/components/Dialog.test.tsx:180-206`; the product code is `app/src/components/Dialog.tsx:97-120`, with the older initial-focus effect at `:53-66`.
- Scenario: the test mounts the dialog with `busy` already true, so `initialFocusDone` is still false. When `busy` turns false, the older initial-focus effect focuses the first control before the new recovery effect runs. The test would pass if the new effect were removed, so it does not protect the fix.
- Suggested fix: mount the dialog idle and let initial focus run, then rerender with `busy` true, then false, and assert focus returns to the first control.

### 4. Low: duplicated focus-row lookup

- Where: `app/src/App.tsx:1652-1663` and `app/src/components/Sidebar.tsx:436-443`.
- Scenario: both places walk `[data-session-id]` and focus the matching row inside a zero-delay timer. The App copy adds a `.session-list` fallback.
- Suggested fix: extract one small helper, such as `focusSessionRow(sessionId, fallback)`, and call it from both paths.

## Checked and correct

### Funnel lifecycle

- The enable runs `tailscale funnel --bg <port>` and keeps the bounded client timeout (`core/src/tailscale.rs:136-150,306-340`). The core and end-to-end fakes assert the `--bg` form, and the end-to-end fake makes a foreground form hang, so a missing `--bg` fails the suite.
- `enable_funnel` holds `demo_lock` across the whole change and re-reads the session after the client command (`core/src/state.rs:2169-2227`). If the session is closing, pausing or has stopped the server, it turns the port off again and returns an error. The race test uses a one-second fake enable and closes the session while it runs.
- `close_session` and `run_pause` turn Funnel off before the agent wait and the Git work (`core/src/api.rs:2827-2829,3729-3735,3754-3757`). The pause path disables again after `begin_close`, so an enable that slipped into the grace period is cleared before the long work.
- A refused turn-off keeps the address and allows a retry for Stop (`core/src/state.rs:2322-2350`, test `demo_servers_funnel_stop_keeps_the_address_when_tailscale_refuses`). Stop returns 422, leaves the server active and public, and the panel shows the reason.
- Close, pause, shutdown and app startup keep the address on failure, and app startup retries it (`core/src/recovery.rs:56-80`). `mark_demo_servers_stopped` no longer clears the address (`core/src/sessions.rs:1925-1932`).
- The Tailscale client runs through `spawn_blocking` in the stop, enable, disable and delete routes (`core/src/api.rs:1121-1183,3906-3916`). The startup sweep runs on a scoped thread (`core/src/recovery.rs:26-31`). The app start can still wait for the startup sweep, but that is not a regression.
- The one-public-server rule holds: an enable turns off every other active public server first, refuses when a turn-off fails, and never records a second public server. A closed record that a turn-off failed to clear is retried on the next start and on delete; the daemon serves one 443 target at a time.
- `public_demo_servers` lists only active sessions, so a closed session's stale address is not treated as a second public server.

### Phone package serving

- The URL is `/ui/<computer>/blake3-<hex>/` (`phone-shell/src-tauri/src/ui.rs:584-599`), and `handle` resolves the build under the named computer only (`:532-555`). Opening X, then Y, then X again returns X's stored build and makes it the served build (`activate`, `:243-273`), which the unit test covers.
- A background update for another computer never changes the served build: `use_build` writes only the per-computer `current.json` and never touches `active` (`:326-359`). The unit test covers this.
- The served build survives two updates: `use_build` reads the served build from `active`, the previous build from `current.json`, and keeps the new build, the previous build and the served build (`:328-352,779-800`). The unit test opens A, applies B and C, and still serves A.
- An incomplete build folder without `index.html` is deleted and the verified temporary folder is renamed into place (`place_build`, `:756-775`). The unit test covers it.
- `current.json` is written when a build is adopted, not only when a new build is downloaded, so a downgrade cannot leave it naming another build (`:326-359`).
- Traversal is refused at the URL path and at the file read: the computer id is limited to ASCII letters, digits, `-` and `_`; the build segment must be `blake3-` plus 64 hex characters; the rest of the path goes through `check_entry_name` and a canonical-path check (`:445-456,501-526,601-624,856-868`). The unit tests cover `..` in the file path, a foreign computer, an unknown build and a non-hash build segment.
- The phone shell Rust tests pass 15 of 15, including the new two-computer, two-update and incomplete-build cases.

### Desktop fixes

- A busy dialog traps Tab in both directions and returns focus inside when a failed request makes the status unmount (`app/src/components/Dialog.tsx:91-158`). The initial focus and the opener restore still work.
- After Move to project, focus goes to the moved session's new row, or to the session list when the row is missing (`app/src/App.tsx:1645-1665`). The end-to-end actions-menu test passes.
- Switching sessions leaves focus on the chosen row; the panel returns focus to its button only when focus was inside the panel (`app/src/components/SessionSidePanel.tsx:130-176`). The unit test and the end-to-end keyboard test pass.
- The header badge shows the work state in words and a shape marker (`app/src/components/StatusBadge.tsx`, `app/src/state/sessionStatus.ts:11-28`). Paused and waiting for input no longer differ only by colour. The end-to-end switching test asserts the visible "Paused" and "Working" text.
- The GitHub repository search keeps the owner and repository name, and a matching recent entry keeps the Added tag (`app/src/components/NewSessionDialog.tsx:150-181`, `app/src/components/Combobox.tsx:18-25,120-132`). The unit test types `acme` and finds the renamed folder.
- Arrow keys move only while a `role="tab"` element has focus, so the browser close and new-tab buttons keep their own behavior (`app/src/components/BrowserPanel.tsx:166-190`). Unit and end-to-end tests pass.
- The file tree puts `aria-selected` on the `role="treeitem"` and keeps the visual state on the button (`app/src/components/SessionSidePanel.tsx:552-571`). Unit and end-to-end tests pass.
- The Funnel confirmation is portaled beside the app frame, the app frame becomes inert while it is open, and focus stays in the dialog (`app/src/components/DemoFunnelDialog.tsx:43-94`, `app/src/App.tsx:454-468,1292`, `app/src/components/SessionSidePanel.tsx:166-174`). Unit and end-to-end tests pass.
- The panel toggle carries `aria-controls` only while the panel exists (`app/src/components/SessionHeader.tsx:99`, end-to-end test).
- The capped allowed list keeps the current config in view and shows a scrollbar (`app/src/components/ConfigMultiSelect.tsx:34-51`, `app/src/styles/app.css:51-54,1650-1672,2359-2368`). It does not scroll a newly added non-current chip into view, but the visible scrollbar makes the list discoverable.
- A saved default branch that is not in the loaded branch list is cleared, explained and re-checked on submit (`app/src/components/NewSessionDialog.tsx:328-354,708-724,1063-1082`). The unit test covers the prefill.
- Repeated question option text gets an index-qualified key (`app/src/components/QuestionForm.tsx:46-49`). The unit test checks that React reports no duplicate key.
- Focus rings draw inside the scrolling tab strips and the allowed list (`app/src/styles/app.css:48-54`, `app/src/styles/phone.css:245-250,334-339`).
- The app unit suite passes 355 of 355 tests in 44 files; typecheck and build pass; the changed end-to-end specs pass 19 of 19.

### Phone fixes

- A failed AI config switch shows an alert inside the open switch sheet and the sheet stays open (`app/src/phone/PhoneApp.tsx:660-670,1300-1314,1983-1987`). The unit test covers it.
- Usage and Close are portaled beside the inert `.phone`, counted in the inert condition, trap Tab through the shared dialog, and return focus to the session-actions trigger (`app/src/phone/PhoneApp.tsx:1298-1322,735-765`). Unit tests cover both dialogs.
- Actions, switch, keys, usage and close sheets close when the active session disappears, and the inert state depends on whether the sheet can render (`app/src/phone/PhoneApp.tsx:397-410,742-756`). The unit test closes the session from the event stream.
- The Start session button stays focusable while it looks disabled, so an invalid submit points at the missing field (`app/src/phone/PhoneApp.tsx:1857-1877`, `app/src/styles/phone.css:327-331`). The React test clicks it; the end-to-end test uses Enter.
- A repeated card wording is announced again: the live region clears and the text is set after a zero-delay timer (`app/src/phone/PhoneApp.tsx:735-751`; the shell uses the same pattern at `phone-shell/ui/src/app.ts:542-561`). Tests check the DOM mutation and the repeated text.
- Closed sessions are out of the All and Waiting filters and out of the phone list (`app/src/phone/list.ts:35-45`). Unit tests on both surfaces cover it.
- A filter with no matches says "No sessions match this filter." instead of "No sessions yet." (`app/src/phone/PhoneApp.tsx:735-899`; shell `phone-shell/ui/src/app.ts:1124-1137`). The new key exists in all 12 desktop catalogs and all 12 shell catalogs.
- After a session starts, the new session title takes focus on both surfaces (`app/src/phone/PhoneApp.tsx:428-436,776-779`; `phone-shell/ui/src/app.ts:649-656,2718-2723`).
- The current view and current list row use `aria-current="true"`, and a waiting active session is marked like the others (`app/src/phone/PhoneApp.tsx:878-884,930-932,2052-2056,2173-2177`).
- The phone app unit suite and the phone shell suite pass; the changed phone end-to-end spec passes; phone shell typecheck, build and clippy pass.

## Tests run

- `npm test`: 355 passed in 44 files.
- `npm run typecheck`, `npm run build`: pass.
- `npm --prefix phone-shell run test`: 128 passed.
- `npm --prefix phone-shell run typecheck`, `npm --prefix phone-shell run build`: pass.
- `cargo test --manifest-path core/Cargo.toml`: 240 passed (216 + 8 + 16), including the five Funnel tests.
- `cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings`: pass.
- `cargo test --manifest-path phone-shell/src-tauri/Cargo.toml --lib ui::`: 15 passed.
- `cargo clippy --manifest-path phone-shell/src-tauri/Cargo.toml --all-targets -- -D warnings`: pass.
- Targeted Playwright specs (`09-switching`, `23-browser`, `24-demo-servers`, `35-keyboard`, `36-phone-keyboard`): 19 passed. The first attempt failed only because the helper uses `ELESDEE_E2E_CORE_URL` while the servers used custom ports; the rerun with matching URLs passed.

## Not verified without a real device or tailnet

- A real Tailscale client accepting `funnel --bg`, the command returning promptly, the daemon replacing the 443 target when another server becomes public, and the saved mapping surviving a daemon restart.
- The phone package in a real mobile webview: the custom protocol on Android (`http://elesdee.localhost`), the marker bridge, keyboard focus and a real screen reader.
- The Funnel confirmation and the other dialogs with a real screen reader.
- A packaged macOS window for the close-to-background and Dock behavior. It is outside this change.

## Review 3 findings not claimed by these commits

- Finding 20 (keys sheet focuses a Remove button or checkbox when opened): `phone-shell/ui/src/app.ts:513-515` still uses one comma-separated selector. Document order decides, and the a11y test still greps the source.
- The second half of finding 21 (browser tabs have no `aria-controls` or tabpanel link): the arrow-key guard is fixed, but the browser tab strip still has no linked tabpanel (`app/src/components/BrowserPanel.tsx:404-465`).
- Finding 24 (a local caller can send `confirmed: true`): unchanged in code. `.dev/SPEC.md` and `.dev/api.md` now state the limitation.
- Finding 25 (startup delay): the client no longer blocks an async runtime worker, but app startup still joins the sweep before the listener starts.
