# Merge review 3: Funnel, accessibility and the UI audit

Range: `e091ea1..dbaf648` on `hiddentao/round-2-mockups`. This covers the Tailscale Funnel demo-server feature, the desktop dialog and keyboard accessibility work, the phone sheet and live-region work, the desktop and phone UI audit fixes, the phone UI package serving change, and the new end-to-end tests.

Method: code reading against `.dev/SPEC.md`, `.dev/api.md`, `.dev/hooks.md`, `.dev/phone-shell.md`, `.dev/providers-configs.md`, `.dev/browser.md` and the upstream Tailscale `serve_v2.go` behavior. No product code was edited. The JavaScript test suites were not run: this worktree has no `node_modules`. The Rust suites were not run either.

## Findings

### 1. High: every "Make public" fails on a current Tailscale client

- Where: `core/src/tailscale.rs:146`. The enable path runs `tailscale funnel <port>` and waits for it to exit (`run_funnel`, 15 second bound at `core/src/tailscale.rs:32, 300-337`).
- Scenario: on current Tailscale, `tailscale funnel <port>` without `--bg` runs in the foreground and prints "Press Ctrl+C to exit"; it never returns. `run_funnel` kills it after 15 seconds and returns `Tailscale did not answer in time.`, which the API maps to 422 (`core/src/api.rs:1165-1175`). So the dialog spins, waits 15 seconds, then reports a failure, and the server stays local. The legacy client (two arguments, `on|off`) rejects the one-argument form, so it fails there as well. The fake client hides this: the core test asserts the wrong argv `["funnel", "3000"]` (`core/tests/api.rs:11522`) and the e2e fake exits 0 for any `funnel` call (`playwright.config.ts:22`).
- Fix: run `tailscale funnel --bg <port>`, check it against a real client, change the core assertion to include `--bg`, and make the fake block without `--bg` so a regression fails the suite.

### 2. High: the phone package server follows the last computer that ran `ensure`, not the computer on screen

- Where: `phone-shell/src-tauri/src/ui.rs:239-261` (`status` returns a cached status without setting `active`; `activate` is just `status`), `:308-319` (`use_build` sets the single global `active`), `:487-499` (`handle` anchors the build root on `active.computer_id`); `phone-shell/src-tauri/src/commands.rs:429-437` (`phone_connect` returns early when a manager exists), `:495-505` (`phone_ui_state` calls `activate`).
- Scenario: two paired computers X and Y. Open X (active X), open Y (active Y), then open X again. `phone_connect(X)` returns the cached manager, `phone_ui_state(X)` returns the cached ready status without touching `active`, and the webview navigates to `/ui/<X-build>/`. `handle` resolves that path under Y's build folder and answers 404 for the entry page and every asset, so the phone shows a dead page. A background reconnect of Y while X's page is open does the same. The single-slot `active` design predates this change, but the new lookup depends on it, and the change only fixes the same-computer update case.
- Fix: make `activate` set `active` from the installed or current build for the requested computer on every call, and do not let another computer's `ensure` replace the build root of an open page. Add a test that opens two computers and activates the first again.

### 3. Medium: closing or pausing during an enable can leave Funnel on after the server stops

- Where: `core/src/state.rs:2155-2204` (`enable_funnel` holds `demo_lock`, runs the blocking client, and never re-checks the session); `core/src/api.rs:2838` (close), `core/src/api.rs:3753` (pause), `core/src/state.rs:2494` and `:2530` call `disable_demo_funnels` without `demo_lock`.
- Scenario: the user confirms Make public, then closes the session while the Tailscale client runs. The close path reads `funnel_address == None`, does nothing, then `finish_close` marks the servers stopped and clears the field. The enable returns and `entry.set_funnel(Some)` writes the public address back onto the closed session. The Tailscale mapping stays on, the record says public on a stopped server, and `public_demo_servers` skips closed sessions, so the next enable does not clear it. It is cleared only by the next app start.
- Fix: take `demo_lock` in the close and pause paths and do the disable and the record change in one critical section; after the client command, re-read the session and roll back (disable and clear) if it is no longer active.

### 4. Medium: a failed Funnel disable discards the only record of a public mapping

- Where: `core/src/state.rs:2264-2278` (`disable_demo_funnels` only logs), `:2296-2305` (Stop logs, then clears), `core/src/sessions.rs:1925-1932` (clears `funnel_address` on stop), `core/src/recovery.rs:67-77` (clears after a failed disable).
- Scenario: the user presses Stop, or closes the session, while Tailscale is stopped, missing or slow. The disable fails and is logged, but the record is cleared anyway. tailscaled can still hold the persisted 443 Funnel mapping. If it returns, or if another process later binds the same loopback port, that port is reachable from the internet while the app shows nothing public and no confirmation was given. Startup recovery has the same behavior, so the mapping can outlive a restart with no trace.
- Fix: keep the record (or a pending-off entry) when the disable fails and retry; for Stop, return an error so the panel shows the reason. Clear only after a successful off.

### 5. Medium: Funnel stays on through the whole close and pause sequence

- Where: `core/src/api.rs:2838` disables only after the agent wait and `git::close_worktree`; `core/src/api.rs:3753` disables only after the 20 second pause grace, the agent exit and `git::pause_worktree`.
- Scenario: a usage pause starts; the entry stays public for the grace period plus a commit and push attempt. A close keeps it public until the worktree close finishes. The agent and its server may already be stopped, and another process that binds the port is exposed. The spec says pause and close turn Funnel off.
- Fix: turn Funnel off at `begin_close` and at the start of the pause, before the long work.

### 6. Medium: a busy dialog lets Tab leave the modal and drops focus when the request fails

- Where: `app/src/components/Dialog.tsx:89-109` (the trap only wraps when focus is exactly the first or last focusable), `:74-78` (the busy effect focuses the sr-only status), `:135-139` (the status is the last child and has `tabindex="-1"`).
- Scenario: submit Rename, Close, Make public or Add project. While busy, focus is on the status paragraph, which is not in `focusableElements`. Tab is not intercepted and moves out of the dialog to the browser chrome. When the request fails, `busy` becomes false, the paragraph unmounts and focus falls to `body`; Escape no longer reaches the dialog until Tab enters it again. This is the accessibility review's busy-Tab finding, still present.
- Fix: compute the next focusable from the full list and clamp when the active element is not in it; when `busy` turns false, restore focus to the first control or the submit button.

### 7. Medium: a successful Move to project loses keyboard focus

- Where: `app/src/components/SessionActionsMenu.tsx:316-328`, `app/src/App.tsx:1125-1167`, `:1640-1647`, `app/src/components/Dialog.tsx:66-72`.
- Scenario: right-click a session, choose Move to project, pick a target. The row is unmounted when the session moves to another project group (`app/src/components/Sidebar.tsx:719-910`). The dialog's captured opener is that row, so `opener.isConnected` is false and no focus restore runs. Focus stays on `body`. Moving the active tab to another project has the same result. The keyboard move path already refocuses the new row.
- Fix: after a successful move, focus the moved session's new row (the `[data-session-id]` lookup used by `confirmKeyboardMove`), or the new active tab.

### 8. Medium: the side panel's focus return fires on every session switch and steals focus

- Where: `app/src/components/SessionSidePanel.tsx:131-144`; the panel is keyed per session at `app/src/App.tsx:1434`, while `SessionHeader` and its toggle button are not keyed (`app/src/App.tsx:1385`).
- Scenario: open the panel for session A, then click or tab to session B. The keyed panel for A unmounts and its cleanup focuses the panel toggle captured at mount. That button is reused for B, so focus jumps from the row or tab the user just activated to the header toggle. The same cleanup runs when the active session is closed or deleted and can override the active-tab focus.
- Fix: restore focus only when the user closed the panel, and only if focus is inside the panel or on `body`.

### 9. Medium: the header badge stopped showing the work state, and input and paused look identical

- Where: `app/src/components/StatusBadge.tsx:11-24` now renders only Active or Closed; `app/src/styles/tokens.css:29-30,99-100` give `input` and `paused` the same colour in both themes; `e2e/09-switching.spec.ts:96-109` was changed from visible text ("Paused", "Working") to the `data-status` attribute.
- Scenario: a paused session and a session waiting for input both show "Active" with the same dot colour. The work state survives only in the hover `title`, which touch and screen-reader users do not get. Before the change the badge showed "Active · Paused" and "Active · Input". This is the WCAG 1.4.1 problem the accessibility review raised, now without the text that partly covered it. The e2e test checks a CSS-derived attribute rather than what the user can see.
- Fix: put the work-state label back as visible text or an `sr-only` label in the badge, and give input and paused distinct tones. Assert the visible label in the e2e.

### 10. Medium: an added GitHub repository disappears from the launch-form search

- Where: `app/src/components/NewSessionDialog.tsx:155-170` (an added repository takes the local folder as its id and the folder basename as its label), `app/src/components/Combobox.tsx:116-129` (the client filters the server result again by label, id and meta), `core/src/github.rs:726-730`.
- Scenario: project `/work/client-app` is a clone of `acme/web`. The core returns `acme/web` with `localFolder: "/work/client-app"`. The option becomes `id` and `meta` `/work/client-app` and `label` `client-app`. Typing `acme` or `web` matches none of those, so the Combobox drops the option and shows "No matches", while the repository is on the computer. Before the change the option kept the full name whenever the folder basename differed, so the search worked. When the local folder is also a recent repository, the GitHub entry is skipped entirely (`:157-159`), so the documented **Added** tag is never shown.
- Fix: keep the GitHub full name in the searchable text (label, meta or a new search field), and merge the Added tag onto the deduped recent option instead of dropping it.

### 11. Medium: a failed phone AI-config switch shows no feedback

- Where: `app/src/phone/PhoneApp.tsx:602-614` (`chooseConfig` sets `actionNotice` but leaves the sheet open), `:692-701` (`anySheetOpen` includes `switchOpen`), `:940-944` (the notice renders inside `.phone`).
- Scenario: tap a config and the core rejects the switch. The notice is inside the inert `.phone`, behind the sheet scrim, so it is neither visible nor announced. The sheet looks unchanged. Before the actions moved into a sheet, the notice was on the visible session screen.
- Fix: show the error inside the switch sheet, or close the sheet and show the notice after the inert wrapper is lifted.

### 12. Medium: the phone Usage and Close dialogs are not modal and lose focus on close

- Where: `app/src/phone/PhoneApp.tsx:1183-1201` (`anySheetOpen` omits `usageOpen` and `closeOpen`), `:1225-1238` (both use the desktop `Dialog` inside `.phone`), `app/src/components/Dialog.tsx:45,68-69`.
- Scenario: open Actions, then Usage. The background is not inert and remains reachable to assistive technology. The dialog captures the actions-sheet row as its opener, but that row unmounts in the same commit as the sheet change, so closing Usage or Close puts focus on `body` instead of the actions trigger. The same happens when the Close dialog is cancelled.
- Fix: portal these dialogs beside `.phone` (as the phone sheets are), include them in the inert condition, and return focus to `#phone-actions-trigger`.

### 13. Medium: the phone can stay permanently inert when the active session disappears with a sheet open

- Where: `app/src/phone/PhoneApp.tsx:384-395` (fallback selection), `:692-698` (`anySheetOpen`), `:1183,1203,1214` (actions, switch and keys sheets are gated on `active`).
- Scenario: the active session is closed from another paired device while the actions, switch or keys sheet is open. `active` becomes null, the gated sheet renders nothing, but `anySheetOpen` stays true, so `.phone` stays inert with no focusable control. If no other active session exists, only a reload recovers.
- Fix: reset those sheet flags when `active` becomes null, and derive the inert state from whether a sheet is actually rendered.

### 14. Medium: the phone launch validation focus path cannot run; the test bypasses the UI

- Where: `app/src/phone/PhoneApp.tsx:1637-1658` (validation branch), `:1768-1772` (submit button disabled by `blocker`), `:1782-1790`; test `app/src/phone/PhoneApp.test.tsx:255-282`.
- Scenario: `startBlocker` returns `incomplete` exactly when `buildStartInput` returns null, so the submit button is disabled whenever the validation branch would run. A disabled default button blocks implicit Enter submission, and a click on it does nothing. A user can never set `aria-invalid` or focus the first invalid field. The test calls `fireEvent.submit(form)`, which no user action can do, so it passes while the feature is unreachable.
- Fix: keep the submit button enabled and validate on submit (the usual accessible pattern), or attach the hint with `aria-describedby` and remove the dead branch; change the test to use the real button or Enter.

### 15. Medium: the phone package prune keeps `current.json`, not the build on screen

- Where: `phone-shell/src-tauri/src/ui.rs:369` (`previous` is read from `current.json`), `:396-401` (write current, then prune), `:711-731` (prune keeps only the new and `previous`), `:308-319` (`use_build` adopts a build without writing `current.json`).
- Scenario: a page is open at build A. The computer updates to B, which is kept as `previous`. While that page is still open, a second update C arrives: `previous` is B, so A is deleted and the open page gets 404 for any asset or route it has not loaded yet. This contradicts the spec sentence the change implements. A downgrade adopted through `use_build` leaves `current.json` naming a different build, so the next install can delete the build being served.
- Fix: treat the build in `active` (the build actually served) as a keep target as well as `current.json`, and write `current.json` when `use_build` adopts a build. Add a test with A active, then B, then C.

### 16. Low: repeated phone cards are announced only the first time

- Where: `app/src/phone/PhoneApp.tsx:672-690` and `:1239-1244`.
- Scenario: when a card is resolved, `announcedCardRef` is reset but the live-region text is not cleared. A second question sets the same string "Question"; a second request sets "Session requests"; a permission for the same tool sets the same "Permission needed: Bash". React sees no change, so no DOM mutation and no announcement.
- Fix: clear the live region when the card key becomes null, or include varying content (the question text or the child session name) in the announcement.

### 17. Low: the phone All filter lists closed sessions that cannot be opened

- Where: `app/src/phone/list.ts:40-42`, `app/src/phone/PhoneApp.tsx:448-462`, `:1100-1120`.
- Scenario: bootstrap includes closed sessions and `all` matches them. Tapping one sets the hash and the session screen, but `active` is null, so the app shows "Choose a session" with no back control. With no open sessions, the phone is stuck on that screen.
- Fix: exclude closed sessions from the phone list or make them non-selectable, or give closed sessions a real view.

### 18. Low: filtering to no matches says "No sessions yet"

- Where: `app/src/phone/PhoneApp.tsx:828-830`.
- Scenario: sessions exist but none match Running or Waiting; the empty text is `phone.noSessions` ("No sessions yet."), which is false.
- Fix: add a separate "No sessions match this filter." key in all 12 catalogs.

### 19. Low: focus is lost after a session starts on the phone

- Where: `app/src/phone/PhoneApp.tsx:1679`, `:199-207`, `:1171-1179`.
- Scenario: the start sheet returns focus to `#phone-start-trigger`, but `onCreated` switches to the session screen and removes that button. Focus stays on `body`.
- Fix: on success focus the session heading or the back button; restore the opener only when the sheet closes without creating.

### 20. Low: the phone keys sheet focuses a Remove button or a checkbox

- Where: `phone-shell/ui/src/app.ts:504-514`.
- Scenario: `querySelector` with a comma-separated selector returns the first matching element in document order, not the first selector that matches. In the keys sheet the saved-key Remove buttons and the Shift/Ctrl/Alt checkboxes precede `#saved-key-name`, so focus lands on a Remove button (Enter removes a key) or on the Shift checkbox. The new a11y test (`phone-shell/test/a11y.test.mjs:13-21`) only greps the source for selector strings, so it cannot catch this.
- Fix: try the preferred selectors in priority order and filter disabled elements; test the actual focus target in the phone layout test.

### 21. Low: tablist arrow handlers react to non-tab controls

- Where: `app/src/components/SessionSidePanel.tsx:478-495` with the panel close button inside the tablist at `:643-651`; `app/src/components/BrowserPanel.tsx:169-193` with the per-tab close buttons at `:434` and the new-tab button at `:451`.
- Scenario: focus the dock close button or a browser tab's close button and press ArrowRight. The handler selects the next tab and moves focus, even though the focus was on a different control. On the browser panel it also starts a tab activation request. The browser tablist also has no `aria-controls`/tabpanel link.
- Fix: ignore the key unless the event target is a `[role="tab"]`; move the non-tab buttons out of the tablist and link the active browser tab to its panel.

### 22. Low: the file tree marks selection on a non-focusable child

- Where: `app/src/components/SessionSidePanel.tsx:524-541`.
- Scenario: focus and roving tabindex are on the `role="treeitem"` `li`, but `aria-current` is on the inner `tabindex="-1"` button. A screen reader on the tree item does not announce the file as selected.
- Fix: put `aria-selected` on the `li` and keep the button style for the visual state.

### 23. Low: the Funnel dialog leaves the background interactive

- Where: `app/src/components/SessionSidePanel.tsx:1074` renders `DemoFunnelDialog` inside the side panel; `app/src/App.tsx:453-467` does not include it in `anyDialogOpen`, and `:1289` marks only the app frame inert.
- Scenario: the confirmation dialog is `aria-modal="true"`, but the page, including the side panel around it, is not inert and stays reachable to assistive technology and pointer users. Every other app-level desktop dialog makes the app frame inert; this one does not.
- Fix: render the dialog as a sibling of the app frame (portal) and add it to `anyDialogOpen`, or move the confirmation to the app-level dialog state.

### 24. Low: the Funnel confirmation is a caller-supplied boolean

- Where: `core/src/api.rs:1139-1151` accepts `{ confirmed: true }`; `app/src/api/client.ts:430-442` always sends it; `.dev/hooks.md:79-85` states that any local program, including code running in a session, can call the local API without an Origin.
- Scenario: a prompt-injected agent or any local process can register a demo server with the MCP tool and then POST the funnel route with `confirmed: true`, exposing a port to the internet without the dialog. This is the one place where a local, unauthenticated caller can make something reachable from the internet.
- Fix: do not treat the boolean as confirmation. Require a one-time token minted by a channel the agent cannot reach (the Tauri shell or a core-side UI confirmation), or keep Funnel out of the local API and expose it only through the shell.

### 25. Low: Tailscale commands block the async runtime and the startup path

- Where: `core/src/api.rs:1139-1163` calls the blocking client from an async handler; `core/src/recovery.rs:26,67` runs the same calls synchronously before the core binds its listener.
- Scenario: with several public records or a slow client, a request worker is held for up to the client timeouts, and app start can be delayed before the local API is available. Today the foreground bug makes every enable take the full 15 second bound.
- Fix: run the client through `tokio::task::spawn_blocking` (or a dedicated thread), and start the listener before the recovery sweep or bound the sweep.

### 26. Low: docs say a failed enable leaves the records unchanged

- Where: `.dev/api.md:388`.
- Scenario: enabling B first turns off A (`.dev/SPEC.md:554`). If B then fails, A's record was already cleared and A is no longer public, so the records changed. The documentation promises the opposite. Stop's failure behavior also differs: `.dev/api.md:389` says Stop clears the address, while the code logs a refused disable and clears it anyway, leaving the daemon mapping behind (finding 4).
- Fix: describe the actual sequence, or restore A when B fails; make the Stop failure visible.

### 27. Low: a repository held by two projects silently resolves to one checkout

- Where: `core/src/github.rs:817-825` (the folder map keeps the last project for a normalized remote), `:1052-1072` (`normalize_remote` drops the host even though its comment says `host/owner/name`), `app/src/components/NewSessionDialog.tsx:544-548`.
- Scenario: two projects clone `acme/web`, for example `/work/web` and `/work/web-experiment`. The map keeps one; the GitHub entry always fills that checkout. A project cloned from another host with the same owner and repository name can also supply the wrong local folder. The old form returned no local folder when more than one folder matched, so it did not silently pick one.
- Fix: keep a set and return a folder only when the normalized remote is unique, or return every matching folder; include the host in the key and fix the comment.

### 28. Low: the collapsed panel toggle points `aria-controls` at an element that does not exist

- Where: `app/src/components/SessionHeader.tsx:99`, `app/src/App.tsx:1432-1440`, `app/src/components/SessionSidePanel.tsx:585`.
- Scenario: with the panel closed, the toggle has `aria-expanded="false"` and `aria-controls="session-side-panel"`, but no element with that id exists because the panel is rendered only when a dock tab is open.
- Fix: always render the panel container and hide it, or add the attribute only while the panel is open.

### 29. Low: the allowed-config cap is a hidden scroll area with no cue or auto-scroll

- Where: `app/src/styles/app.css:2332-2340`.
- Scenario: the chips fill two rows and the user adds another config. The new chip is appended below the 60 px viewport. The list is not included in the persistent scrollbar rules (`app.css:1623-1645`), and nothing scrolls it into view, so the user cannot see the new chip or the "Current" marker without discovering the scroll. The value is still submitted, so nothing is lost.
- Fix: add the list to the scrollbar rules, scroll the new chip into view, or show a count or expand control.

### 30. Low: a stale project default branch can be submitted

- Where: `app/src/components/NewSessionDialog.tsx:265-267`, `:468-478`, `:675-680`.
- Scenario: a project stores a default source branch that was deleted or renamed. The form prefills it without checking the loaded branch list and submits it; the launch fails at the core. Changing the folder clears the value, so only the initial and project-change prefills are affected. This predates the reviewed commits but sits on the branch path they touched.
- Fix: clear or flag the prefilled branch when the loaded branch list does not contain it.

### 31. Low: duplicate question option text gives duplicate React keys

- Where: `app/src/components/QuestionForm.tsx:46-48`.
- Scenario: an agent asks a question with the options `["Red", "Red"]`. The map uses `key={option}`, so React gets two identical keys and may reuse the wrong button.
- Fix: include the index in the key.

### 32. Low: a partial build directory is adopted when it has no entry page

- Where: `phone-shell/src-tauri/src/ui.rs:390-395`.
- Scenario: `find_build` only accepts a directory with `index.html` (`:461-473`), so this branch is reached when an existing directory lacks one, for example a build left partly deleted when a previous prune was interrupted. The code deletes the verified temporary unpack and adopts the incomplete directory, writes `current.json`, and reports ready. The page has no entry page, and every later connect repeats the full download and fails the same way.
- Fix: remove the existing directory and rename the verified temporary build into place.

### 33. Low: the phone marks tabs as "page" and never marks a waiting active session

- Where: `app/src/phone/PhoneApp.tsx:811-818` (the waiting section renders `SessionRow` without `active`), `:860-862` and `:2081` (`aria-current="page"` on tab-like buttons).
- Scenario: the active session is waiting. It appears in the waiting section without the active class, check mark or `aria-current`, while the same session in the "everything else" section would have them. Tab-like controls also announce "current page" rather than a tab selection. The attribute is at least on the clickable element.
- Fix: pass `active` to the waiting rows, and use tab semantics (`role="tab"` with `aria-selected`) or `aria-current="true"` for the view switchers.

### 34. Low: focus rings are clipped on the phone swipe rows and tab strip

- Where: `app/src/styles/phone.css:320-324` (`.phone-swipe-row` has `overflow: hidden`) and `:215-222` (`.phone-tabs` has `overflow-x: auto`); the global ring is outside the element at `app/src/styles/app.css:42-45`.
- Scenario: a keyboard user tabs through the tab sheet or the swipe rows; the outer outline and box shadow are clipped on the exposed edges, so the focused row has no visible indicator. The active row still differs by background, but a non-active row does not.
- Fix: use an inset focus style for these rows (as `.phone-swipe-close` already does) or draw the ring inside the row.

### 35. Low: the new end-to-end tests leave gaps

- Where: `e2e/24-demo-servers.spec.ts:149-226` (one server only), `e2e/34-notifications.spec.ts:90-145` (never asserts `sessionVisible`), `e2e/24-phone-tab-close.spec.ts:61-64` (the list fallback also passes), `e2e/03-session-menu.spec.ts:30-35,75-81` (presence checks replaced the full menu), `e2e/32-desktop-layout.spec.ts:226-237` (a missing add-tab box becomes `0` and passes; the "same key" filter is dead), `e2e/32-settings-diagnostics.spec.ts:97-102` (`>= 0` counts and an empty log array pass), `playwright.config.ts:22` (the fake records no arguments, so a wrong port would pass).
- Scenario: the one-public-server rule, the pause and restart cleanup, the notification suppression flag, the tab-close result and the diagnostics content can all regress while the suite stays green. The fake Tailscale client cannot catch the missing `--bg` or the exposed port.
- Fix: add the two-server enable case, a pause and restart case, and assert the daemon arguments; assert `sessionVisible` for the focused and unfocused cases; assert the surviving tab name and the tab bar; restore an exact menu assertion; assert the boxes are non-null and the counts and log content.

## Checked and correct

- The Funnel route takes only a session id and a demo-server id, requires the server and session to be active, refuses a stopped server with 422, and maps an unknown session or server to 404. Stop, close, pause, delete and app startup all call the disable path, and the public address is persisted so startup can clear it.
- The one-public-server rule is enforced in code: enabling a server disables and clears every other public server first, and the demo lock serialises concurrent enables.
- An app-level desktop dialog sets initial focus, returns focus to a connected opener, closes on Escape, and makes the app frame inert while it is open (the Funnel dialog is the exception, finding 23). The dock and browser tab models have one roving tab stop and Arrow/Home/End handling. The file tree has one roving stop, expansion state, the full arrow model and Enter/Space activation. The session menu closes, restores the trigger and handles all arrow keys without double handling.
- Drag uses pointer capture, a threshold and a click-suppression flag, so a drag does not also select the session. The menu, drag and `M` paths cannot run at once.
- The phone sheets are portaled outside the inert page, take focus, trap Tab and Shift+Tab, close on Escape and restore focus to the trigger in the normal close paths. The live region is mounted once and is not rewritten on unrelated session polls. `list.ts` is used by the phone app and its status filter matches the documented meaning.
- The phone package store checks the hash and size before unpacking, refuses absolute paths, `..`, backslashes, symlinks, duplicate names and oversized entries, confirms the canonical target stays in the build folder, and sets a CSP and immutable caching on hashed assets. A failed download or unpack leaves the previous build active.
- The GitHub repository result carries `localFolder` from the same remote map that sets `added`, so two different repositories with the same folder name stay apart (two checkouts of the same repository are finding 27). The copy actions moved cleanly to the branch chip, and the no-branch session menu keeps a folder copy action.
- The new e2e specs run under the existing `testDir` and CI job, use `workers: 1`, and introduce no fixed sleeps. The changed OpenCode, Codex, session-menu and phone-action specs match the current markup.
