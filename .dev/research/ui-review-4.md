# UI review 4: desktop shell, sidebar, session header, side panel, Settings and phone

Target: commit `d9a6dda`. I did not edit code. The core was not stopped or changed.

Method: the app ran at `http://127.0.0.1:5173` and the phone layout at `/phone.html`. I injected a fixed bootstrap with four projects, five providers, five AI configs, eleven sessions (working, waiting, paused, done, closed and terminal), one question and one request. File, diff, devices, branch and browser responses were stubbed in the browser context. Screenshots are in `/private/tmp/elesdee-r4/`.

Screens and widths:

- Desktop: 1440x900 and 1024x768, light and dark.
- Phone: 390x844, light and dark.
- States: running, permission, question, paused, closed, collapsed, usage popover, session menu, branch menu, Questions, Requests, new session, first launch, add project, Settings sections, side panel Files, file editor, Changes, Browser, and settings sub-dialogs.

Result: no page-level horizontal scroll at 1440, 1024 or 390. No console output in the phone, first-launch or settings flows. One console error repeats on the desktop whenever the session side panel opens: a duplicate React key (finding 10). Both themes render with the warm direction B palette. No contrast failure was found for the body and secondary text tokens in either theme.

## Findings

### 1. High: the phone has no usable empty state and the start button dead-ends

Where: phone list, 390 wide, light and dark. Also the New session sheet when no projects exist.

Observed: with no projects, the list still shows the All, Running and Waiting filters and the single line `No sessions yet.` The plus button opens a New session sheet with empty Project and AI config combos and an enabled `Start session` button. There is no message that a project must be added on the computer first, and the phone has no way to add one.

Evidence: `phone-empty-390-light.png`, `phone-empty-390-dark.png`, `phone-empty-start-sheet-390-light.png`.

Minimal fix: when `projects.length === 0`, hide the filters and the plus, and show one empty card naming the next step ("Add a project on the computer to start a session"). In `StartSessionSheet`, disable the submit button until a project and an AI config are chosen, and say why it is disabled. See `app/src/phone/PhoneApp.tsx:595`, `:625`, `:669` and `:1418`.

### 2. High: the phone "Running" filter includes paused and finished sessions

Where: phone list, 390 wide, light and dark.

Observed: selecting `Running` shows the paused pricing session and the finished session-state session. The predicate is `status === 'active' && !isWaiting`, so every active session that is not waiting passes, including paused and done. The spec and the mockup use Active for the two-state model. Running is a work state inside Active, not a state.

Evidence: `phone-filter-running-390-light.png`: the list contains `Pricing page token migration` with `Paused` and `Session state migration` with `Finished`.

Minimal fix: label the filter `Active` and show every active session, or keep the label `Running` and require `workState === 'working'`. Use the same wording as the desktop sidebar. See `app/src/phone/PhoneApp.tsx:550`.

### 3. Medium: phone waiting rows repeat "Waiting for you" and drop the useful preview

Where: phone list, 390 wide, light and dark.

Observed: each waiting row shows `Waiting for you` in the preview line and `Waiting for you` again as the right-hand state. The session preview is discarded even when it says what is needed, such as `Waiting to push the fix`. The section heading already says `Waiting for you`. The same text appears three times in one row region.

Evidence: `phone-list-390-light.png`, `phone-list-390-dark.png`.

Minimal fix: keep the session preview (or the waiting reason) in the preview line and use `Waiting` as the right-hand state. The desktop row already keeps `Active` on the right, so the phone can do the same. See `app/src/phone/PhoneApp.tsx:1725` and `:1731`.

### 4. Medium: the phone key rail overflows with a hidden scrollbar

Where: phone session, 390 wide, light and dark.

Observed: the rail is 370 CSS px wide and 568 px of content. The down arrow is cut at the right edge; the right arrow, `^C`, `^D` and the add-key button are fully off screen. The scrollbar is hidden. Nothing in the rail shows that it scrolls except the cut key.

Evidence: `phone-session-messages-390-light.png`, `phone-session-terminal-390-light.png`, `phone-session-messages-390-dark.png`.

Minimal fix: reduce the key min-width and horizontal padding, or let the key rows wrap to a second row. Keep the latch row and the hidden scrollbar only as a fallback. See `app/src/styles/phone.css:636` and the 40px minimum at `:650`.

### 5. Medium: the phone stacks about 260px of controls before the conversation

Where: phone session, 390 wide, light and dark.

Observed: below the 65px header, the session adds a 56px tab strip, a 34px row of three full-width action buttons (`Switch AI config…`, `Usage`, `Close…`), a 50px row of `Files` and `Changes`, and a 44px context strip. The message view starts at 265px. The controls from the header to the context strip take 257px, or 30% of the screen. The mockup puts provider context and one Terminal toggle under the header (about 150px) and keeps the remaining actions in the overflow menu. The functions are required; the stacked full-width rows are the problem.

Evidence: `phone-session-messages-390-light.png`, `mockup-phone-session.png`.

Minimal fix: move Switch AI config, Usage and Close into the header overflow or the tab sheet. Put Files and Changes behind one secondary control, or combine them with the Messages and Terminal toggle. Keep the tab sheet for navigation. Collapse the controls to one or two compact rows so the conversation gets most of the screen.

### 6. Medium: the New session form hides fields below the fold at 1440 and 1024

Where: New session dialog, 1440x900 and 1024x768, light and dark.

Observed: the form body is 783px tall. At 1024 it gets 584px of visible height, and at 1440 the `Source branch` field is already cut by the footer. `Session name` and everything in Additional settings (first prompt, environment variables) require an internal scroll. The scrollbar is hidden on macOS until the user scrolls, and the footer cuts the field rather than showing a fade.

Evidence: `desktop-new-session-1440-light.png`, `desktop-new-session-1024-light.png`.

Minimal fix: compact the allowed-config block (it takes about 100px), reduce the 16px field gaps, and keep Additional settings closed by default. Confirm that the first screenful at 1024 shows Project, Session type, AI config, repository and source branch without a partial field under the footer. Add a visible scroll cue to the dialog body. See `app/src/components/NewSessionDialog.tsx:1047` and `app/src/styles/app.css:1572` and `:1614`.

### 7. Medium: the side panel is too narrow at 1024 and truncates its context

Where: session side panel, 1440 and 1024, light and dark.

Observed: the dock is 340px at 1440, 300px at 1200 or less, and 276px at 1024. The mockup direction B dock is 380px. At 1024:

- Session name shows 96px of 129px and has no `title`.
- Branch shows 141px of 190px; it has a `title`.
- Demo server name shows 33px of 47px; the address shows 101px of 145px, and the address has no `title`.
- The diff pane is 253px wide with 345px of content and a hidden horizontal scrollbar.

Evidence: `desktop-panel-files-1024-light.png`, `desktop-panel-changes-1024-light.png`, `desktop-panel-changes-1440-light.png`.

Minimal fix: use the mockup's 380px dock at wide widths and keep a 320px floor. Below 1200, put the session name and branch on two lines instead of one truncated line. Add `title` or `aria-label` to the session name and demo address. Let a diff wrap, or open it at full main width. See `app/src/styles/app.css:3377`, `:4189`, `:4195`, and `app/src/components/SessionSidePanel.tsx:467`, `:515`.

### 8. Medium: usage limit labels are cut to "5-hour ..." and "Weekly ..."

Where: sidebar usage popover and the provider pill tooltip, 1440 and 1024, light and dark.

Observed: the grid gives the label column 52px. `5-hour limit` needs 61px and `Weekly limit` needs 63px, so both show an ellipsis. This is visible in the main usage popover, which is the screen for reading limits.

Evidence: `desktop-usage-popover-1440-light.png`, `desktop-usage-popover-1440-dark.png`.

Minimal fix: widen the label column to about 68px, or shorten the English labels to `5-hour` and `Weekly` and update all 12 catalogs. See `app/src/styles/app.css:1959`.

### 9. Medium: session names truncate at 1024 and the row tooltip does not contain the name

Where: sidebar, 1024 wide, light and dark.

Observed: the name column is 170px. `Pricing page token migration`, `Rewrite provider usage docs` and `Android QR pairing screen` truncate. The row `title` is only `Active, Paused` or `Active, Working`, so hovering does not reveal the full name. At 1440 the names fit.

Evidence: `desktop-running-1024-light.png`, `desktop-running-1024-dark.png`.

Minimal fix: include the session name in `title` or `aria-label` (for example `Pricing page token migration, Active, Paused`). If the name should fit, hide the right-hand status label below 1120 or reduce the row gaps. See `app/src/components/Sidebar.tsx:119` and `:166`.

### 10. Medium: duplicate React keys when the side panel opens

Where: desktop session with the side panel open, any width, both themes.

Observed: React logs `Encountered two children with the same key, reconnect-backoff` each time the panel opens. `SessionView` and `SessionSidePanel` are siblings and both use `key={activeSession.id}`. The app still renders, but React states that duplicate keys can duplicate or omit children.

Evidence: console capture during `desktop-panel-files-1440-light.png`.

Minimal fix: give each child a distinct key (`view:${id}` and `panel:${id}`) or remove the key from one child. See `app/src/App.tsx:1401` and `:1428`.

### 11. Medium: copy-folder and copy-branch exist in two menus

Where: session header branch menu and the overflow menu, any width.

Observed: the branch pill menu lists `Copy branch name` and `Copy folder path`. The overflow menu lists the same two actions. AGENTS says not to add redundant interface elements.

Evidence: `desktop-branch-menu-1440-light.png`, `desktop-session-menu-1440-light.png`.

Minimal fix: keep the copy actions in the branch pill menu and remove them from the overflow menu, or keep them in the overflow and remove the branch menu. See `app/src/components/SessionActionsMenu.tsx:76` and `:80` and `app/src/components/BranchActionsMenu.tsx:43` and `:46`.

### 12. Low: the tab-strip plus is pinned to the far right

Where: desktop tab strip, 1440 and 1024, light and dark.

Observed: the plus sits at the right edge of the window, with a large empty strip between the last tab and the button. The mockup note says the plus sits after the last tab.

Evidence: `desktop-running-1440-light.png`, `desktop-running-1024-light.png`.

Minimal fix: render the add button inside the `.tabs` row after the mapped tabs, or position it adjacent to the last tab when there is spare width. See `app/src/components/TabStrip.tsx:129` and `app/src/styles/app.css:879`.

### 13. Low: Settings is an inset modal, not the mockup's full-page view

Where: Settings, 1440 and 1024, light and dark.

Observed: the settings surface has a 24px margin around it and a scrim over the app. The mockup replaces the window content with a full-bleed Settings page, with no inset and no dimmed app behind. The app's near-full-screen modal also leaves the titlebar and New session button visible through the top edge.

Evidence: `desktop-settings-providers-1440-light.png`, `mockup-settings.png`.

Minimal fix: at widths of 720 or more, render Settings edge to edge without the dialog inset or scrim. Keep the current sheet at phone width. See `app/src/styles/app.css:1557` and `:2330`.

### 14. Low: "Used by 1 AI configs" and "1 arguments"

Where: Settings > Providers and the AI config summary, all widths and themes. The string appears once per provider card.

Observed: the account line uses one catalog string for every count, so a provider used by one config reads `Used by 1 AI configs`. The config summary has the same shape for `1 arguments`.

Evidence: `desktop-settings-providers-1440-light.png`.

Minimal fix: add one and many forms for `providers.usedBy` and `configs.argsCount` in all 12 catalogs, and choose the form from the count. See `app/src/components/SettingsDialog.tsx:652` and `app/src/i18n/en.ts:821`, `:901`.

### 15. Low: the row status label is always gray and the header badge adds a second state

Where: sidebar and session header, all widths, both themes.

Observed: the right-hand `Active` or `Closed` label uses `--text-3` for every status. The mockup colours the label with the status tone. The header badge is also 124px wide and reads `Active · Working` or `Active · Waiting for you`, while the mockup badge is about 66px and reads only `Active`.

Evidence: `desktop-running-1440-light.png`, `mockup-b-running.png`.

Minimal fix: colour the row label with the status tone, and keep only Active or Closed in the badge. The detail is already present in the row preview and the tab sheet, so the detail word can move to the title attribute. See `app/src/styles/app.css:765` and `:968`, and `app/src/components/StatusBadge.tsx:24`.

### 16. Low: the phone tab sheet replaces the active session state with "Open"

Where: phone tab sheet, 390 wide, light and dark.

Observed: every row shows its work state (`Working`, `Paused`, `Finished`, `Waiting`) except the active row, which shows `Open`. The active row is already highlighted, so the word replaces information rather than adding it.

Evidence: `phone-tabs-sheet-390-light.png`.

Minimal fix: keep the work state on every row and mark the active row with the highlight and a check. See `app/src/phone/PhoneApp.tsx:1858`.

### 17. Low: phone list rows omit project and client

Where: phone list, 390 wide, light and dark.

Observed: the row shows the session name, one preview line and the state. The mockup adds `web, Claude Code` under the name. With several sessions in one project this is fine; with sessions from different projects, the row no longer says where it runs or which agent it uses.

Evidence: `phone-list-390-light.png`, `mockup-phone-list.png`.

Minimal fix: add a compact meta line with project and client, or put the project at the start of the preview line. If row height must not grow, use the project in the preview line and client in the state title. See `app/src/phone/PhoneApp.tsx:1725`.

### 18. Low: the usage popover exposes raw vendor slugs and cuts the last card

Where: sidebar usage popover, 1440 and 1024, both themes.

Observed: the card header shows `work · anthropic`, `pay as you go · deepseek` and `Codex · openai`. These are vendor identifiers, not the labels used elsewhere in the app. The fifth provider card is cut mid-row at the popover bottom, and the popover has a hidden scrollbar, so it is not clear that more content exists.

Evidence: `desktop-usage-popover-1440-light.png`, `desktop-usage-popover-1440-dark.png`.

Minimal fix: use the provider option label or omit the vendor from the header. Add a bottom fade or a `More` control when the list scrolls. See `app/src/components/UsagePopover.tsx` and `app/src/components/UsageLimitRow.tsx`.

### 19. Low: Questions and Requests dialogs have two primary actions

Where: Questions and Session requests dialogs, 1440 and 1024, light and dark.

Observed: in the Requests dialog, `Approve` and the footer `Close` both use the dark primary style, so the dialog has two primary actions. In the Questions dialog, the footer `Close` is the only primary and it receives focus on open, while answering an option is the task. A design lead would ask for one clear primary action.

Evidence: `desktop-questions-dialog-1440-light.png`, `desktop-requests-dialog-1440-light.png`.

Minimal fix: make the footer Close a ghost or secondary button in both dialogs. Keep Approve as the only primary action in Requests. In Questions, make the answer options the primary style and focus the first option or the question, not Close. See the dialog footers in `app/src/components/QuestionsDialog.tsx` and `app/src/components/RequestsDialog.tsx`.

### 20. Low: the phone tab and language sheets have no visible close control

Where: phone tab sheet and Languages sheet, 390 wide, light and dark.

Observed: both sheets can be dismissed only by tapping the scrim. The tab sheet has no handle or close button; the Languages sheet has no close button. The mockup sheet has a handle and a close control.

Evidence: `phone-tabs-sheet-390-light.png`, `phone-language-sheet-390-light.png`.

Minimal fix: add a small close button or drag handle to both sheets, and keep the scrim tap. See the sheet headers in `app/src/phone/PhoneApp.tsx` around `:700` and `:1000`.

### 21. Low: the closed and paused banners omit part of the state

Where: closed and paused session, 1440 and 1024, both themes.

Observed: the closed banner says the log is kept and the branch stays, but not that the worktree was removed or when the session closed. The paused banner repeats the preview and offers Resume, but does not say that work is committed and pushed or that it resumes when usage returns. The mockup gives those facts.

Evidence: `desktop-closed-1440-light.png`, `desktop-paused-1440-light.png`.

Minimal fix: include the close time and worktree state in the closed banner, and the commit and resume conditions in the paused banner. Keep the copy in all 12 catalogs.

### 22. Low: the session header puts the provider before the branch and uses one chip shape

Where: session header, 1440 and 1024, both themes.

Observed: the metadata row shows the provider pill first and the branch chip second. Both are 999px pills. The mockup puts the branch first as a 6px rounded rectangle and the provider second as a pill. The app branch chip is 235px wide and the provider 219px; the order changes which one the eye reads first, and the branch no longer looks like metadata.

Evidence: `desktop-running-1440-light.png`, `mockup-b-running.png`.

Minimal fix: render the branch chip before the provider, set the branch radius to `var(--radius-sm)` (6px), and keep the provider pill. See `app/src/components/SessionHeader.tsx:55` and `app/src/styles/app.css` under `.chip-meta` and `.branch-pill`.

## Checked with no confirmed mismatch

- Page-level horizontal scroll: none at 1440, 1024 or 390.
- Console: clean in the phone, first launch and settings flows; only the duplicate key in finding 10.
- Shell dimensions: titlebar 47px, tab strip 37px, tab 36px, session head 68.5px and session title 20px now match direction B closely.
- Session rows are 38.8px with two lines and a right-hand status label, close to the mockup's 36px.
- Direction B palette, warm surfaces, rust accent, status tones and terminal panel match in both themes.
- First launch: filters and usage are hidden, agents are listed with versions, and Add project is the single action.
- Closed, paused, permission and question states are present and do not clip at 1440.
- Side panel file tree, editor, save state and changes list render; the diff is readable at 1440 and scrolls horizontally at 1024.
- Settings sections render at both widths without page scroll. The settings nav and cards fit.
- Phone list, session, message view, terminal view, key rail, tab sheet and language sheet render without page-level horizontal scroll.
- Dark mode contrast: the body and secondary text tokens pass 4.5:1; no dark-specific layout break found.

## Screenshot inventory

Desktop light:

- `desktop-running-1440-light.png`
- `desktop-panel-files-1440-light.png`
- `desktop-panel-changes-1440-light.png`
- `desktop-panel-file-readme-1440-light.png`
- `desktop-panel-browser-1440-light.png`
- `desktop-permission-1440-light.png`
- `desktop-question-1440-light.png`
- `desktop-paused-1440-light.png`
- `desktop-closed-1440-light.png`
- `desktop-session-menu-1440-light.png`
- `desktop-branch-menu-1440-light.png`
- `desktop-usage-popover-1440-light.png`
- `desktop-questions-dialog-1440-light.png`
- `desktop-requests-dialog-1440-light.png`
- `desktop-new-session-1440-light.png`
- `desktop-settings-providers-1440-light.png`
- `desktop-settings-general-1440-light.png`
- `desktop-settings-ai configs-1440-light.png`
- `desktop-settings-languages-1440-light.png`
- `desktop-settings-devices-1440-light.png`
- `desktop-settings-diagnostics-1440-light.png`
- `desktop-settings-about-1440-light.png`
- `settings-add-provider-1440-light.png`
- `settings-add-config-1440-light.png`

Desktop dark:

- `desktop-running-1440-dark.png`
- `desktop-panel-files-1440-dark.png`
- `desktop-panel-changes-1440-dark.png`
- `desktop-permission-1440-dark.png`
- `desktop-usage-popover-1440-dark.png`
- `desktop-new-session-1440-dark.png`
- `desktop-settings-providers-1440-dark.png`
- `desktop-settings-general-1440-dark.png`

Desktop 1024:

- `desktop-running-1024-light.png`
- `desktop-panel-files-1024-light.png`
- `desktop-panel-changes-1024-light.png`
- `desktop-panel-file-readme-1024-light.png`
- `desktop-permission-1024-light.png`
- `desktop-settings-providers-1024-light.png`
- `desktop-settings-devices-1024-light.png`
- `desktop-new-session-1024-light.png`
- `desktop-running-1024-dark.png`
- `desktop-new-session-1024-dark.png`
- `desktop-panel-files-1024-dark.png`
- `desktop-settings-providers-1024-dark.png`

First launch:

- `first-launch-1440-light.png`
- `first-launch-1440-dark.png`
- `first-launch-1024-light.png`
- `first-launch-add-project-1440-light.png`
- `first-launch-add-project-github-1440-light.png`

Phone, 390:

- `phone-list-390-light.png`
- `phone-permission-390-light.png`
- `phone-question-390-light.png`
- `phone-session-messages-390-light.png`
- `phone-session-terminal-390-light.png`
- `phone-tabs-sheet-390-light.png`
- `phone-language-sheet-390-light.png`
- `phone-filter-running-390-light.png`
- `phone-list-390-dark.png`
- `phone-session-messages-390-dark.png`
- `phone-session-terminal-390-dark.png`
- `phone-permission-390-dark.png`
- `phone-empty-390-light.png`
- `phone-empty-390-dark.png`
- `phone-empty-start-sheet-390-light.png`

Reference:

- `mockup-b-running.png`
- `mockup-settings.png`
- `mockup-phone-list.png`
- `mockup-phone-session.png`
