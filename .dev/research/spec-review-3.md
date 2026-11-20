# Spec review 3: merged features at b19b066

Target: commit `b19b066`. I read `.dev/SPEC.md` and `docs/`, and inspected the code at that commit. I did not edit code. Line numbers refer to `b19b066`. The phone transport and client-update work in flight is excluded.

## Findings

### 1. High: a session that waits for plain input sends no phone notification

Spec lines 597 and `docs/phone.md:58` say a notification comes when a session needs input, a permission answer or an approval.

The phone notice decision requires a waiting state:

- `phone-shell/ui/src/notifications.ts:111` returns early when `session.waitingFor` is null.
- `core/src/api.rs:4887-4892` handles only permission notifications from Claude Code. Other notification messages are ignored.
- `core/src/state.rs:55-60` defines `WaitingFor::Input`, but no code in `core/src` assigns it. The only `WorkState::Input` assignment is `core/src/sessions.rs:749`, in `mark_switch_failed`, which is an error state that the spec says must not notify.
- `phone-shell/test/notifications.test.mjs:69` builds a session with `waitingFor: 'input'`, so the unit test passes, but the real event path never produces that state.

The same condition exists in the desktop notifier at `app/src/App.tsx:228-232`. An agent that stops and waits at a prompt therefore sends neither a phone notice nor a desktop notice.

Minimal fix: set `waitingFor: Input` from the client's idle or input signal (for Claude Code, a non-permission `Notification`; for the other clients, their equivalent), clear it on user input and tool use, and keep the failed-switch error state separate. Add an integration test from the hook to the phone notice.

### 2. High: browser console logs do not redact addresses or credentials

Spec line 262 and `docs/sessions.md:70` say both browser logs hide login details, sensitive address values and the keys the app knows.

Network URLs are redacted at `core/src/browser/logs.rs:84`, but console entries are only cleaned and capped:

- `core/src/browser/logs.rs:68-73` calls `clean` and `cap`, not `redact_url`.
- `core/src/mcp.rs:1111-1117` removes app-known secret values, then returns the console text unchanged.

A page that logs `fetch("https://user:pass@example.test/path?token=secret")` sends the login details and the query value to the agent through `browser_console`.

Minimal fix: run the console text through the same URL-aware redaction used for network addresses before it leaves the core. Share the userinfo and sensitive-query rules, and add a test with a URL inside a console message.

### 3. Medium: the launch form has no GitHub repository search

Spec line 473 says the launch form shows the repository and I can pick a repository from recent sessions or search GitHub after connecting.

`app/src/components/NewSessionDialog.tsx:291-312` returns only `recentRepositoryOptions`. The dialog imports no GitHub action. The GitHub search and clone functions already exist in `app/src/api/client.ts:343-357` and are used by `app/src/components/AddProjectDialog.tsx:15`; `docs/sessions.md:30` also describes only recent repositories and a typed path.

Minimal fix: when GitHub is connected, merge `listGitHubRepos(query)` into the repository combobox and use the existing clone flow when the chosen repository has no local folder. Update `docs/sessions.md` step 6 in the same change.

### 4. Medium: recent repositories come from the current project, not the repository a session used

`docs/sessions.md:30` says the list shows repositories used by earlier sessions.

`app/src/components/NewSessionDialog.tsx:77-86` maps each session to `projectsById.get(session.projectId)?.folder`. The session record has no source repository field (`core/src/state.rs:153-175`). For an agent session, `Session.folder` becomes the worktree at `core/src/sessions.rs:566-572`, after the source repository is resolved at `core/src/api.rs:5234-5237`. Moving a session changes only its project at `app/src/App.tsx:1129-1140`.

As a result, a session started from a custom repository is listed under the project folder, and a moved session is listed under the target project folder, even though its worktree and branch came from the source repository. The source repository is held only in memory on the session entry.

Minimal fix: save the resolved source repository on the session record at create, expose it in the session API, and use it in `recentRepositoryOptions`. Fall back to the project folder for records without the field.

### 5. Medium-low: the live browser view drops every modified key

Spec line 263 and `docs/sessions.md:64` say my keys go to the session's browser.

`app/src/components/BrowserPanel.tsx:200-203` returns immediately when Ctrl, Meta or Alt is held. Only unmodified printable characters and the named keys in `LIVE_KEYS` reach the page. Page shortcuts such as Ctrl+C, Ctrl+A, Cmd+F and paste do not work in the live view.

Minimal fix: send modified key events with a modifier mask, and reserve the app's own shortcuts. The current key path in `core/src/browser/chromium.rs` is limited to named keys, so it also needs key definitions for letters and digits. If the limitation is intended, state it in the interface and the docs.

### 6. Low: an image over the cap is reported as "not an image"

`docs/phone.md:66` distinguishes a binary file, an unsupported image and an image over the size cap, and says each shows a short explanation.

Both phone surfaces catch every image read failure and show one message:

- `app/src/phone/SessionFiles.tsx:152-160` shows `phone.fileNotImage` for every image error, including a 413 from `core/src/files.rs:267-274`.
- `phone-shell/ui/src/app.ts:1776-1785` shows `files.notImage` for every image error, including the `too_large` code.

The preview choice also uses the file extension (`app/src/phone/files.ts:4-26`), while the core detects image types from the signature (`core/src/files.rs:290`). A PNG with no extension is read as text and reported as "not text."

Minimal fix: map `413` and `too_large` to a size-specific message, keep the unsupported-image message for `415`, and add the keys to all 12 catalogs. Optionally try the image read when the text read reports a binary file.

### 7. Low, documentation: notification permission timing

Spec line 592 and `docs/phone.md:60` say the app asks for the notification permission on first use. `phone-shell/ui/src/app.ts:398` calls `ensureNotifications` during `start`, so the shell asks when the app opens. `.dev/phone-shell.md` says start, so the code follows that design.

Minimal fix: make the spec and public docs say "when the app starts," or move the request to the first notice. Do not leave the two statements different.

## Checked with no confirmed mismatch

- Close to background: on macOS the close button hides the window (`src-tauri/src/main.rs:339-350`), the core keeps running, `ExitRequested` shows the window and asks before quitting, and `Reopen` shows the same window (`src-tauri/src/main.rs:383-400`). This matches the docs for the v1 platform.
- Prepaid minimum balance: a new or sample DeepSeek provider has the default 5 (`core/src/api.rs:1350-1354`, `core/src/seed.rs:59`), the field appears only for a balance source, a balance at or below the minimum is `NearLimit` (`core/src/providers.rs:378-385`), and the value is saved with the provider. The auto-switch candidate filter excludes providers without room.
- Phone files and uploads: the core enforces 25 MiB, private file and folder modes, name cleaning, no overwrite and numbered suffixes, writes the upload outside the repository, and tells the agent the path. The image route detects PNG, JPEG, GIF, WebP, AVIF and BMP from the signature. The phone lists one folder at a time and shows text, images and diffs. Only the error-message issue above was found.
- Phone UI package delivery: the package build is deterministic, the core checks the manifest, hash, size and entry names, the shell checks the download again, refuses links and unsafe paths, keeps the current and previous builds, and gates by protocol and bridge versions. The custom protocol serves only the active build. The docs already say no device test has run.
- Session dragging: the pointer threshold, dashed target, Escape cancel, keyboard M, arrow selection, Enter and Escape, tab move between projects, and backend project validation match `docs/sessions.md:40-46`. The backend changes only the project and keeps the folder, branch, worktree and log.
- Launch form grouping: the first prompt, environment variables and uncommitted-change choice are under Additional settings (`app/src/components/NewSessionDialog.tsx:762-842`). The two issues above are the repository list and the missing GitHub search.
- Phone notification suppression and routing: waiting and done notices stay quiet when the session is open and the app is in front, messages always notify, the shell keeps the event stream open on the computer list, and a tap targets the parent session for a request. The input trigger above is the gap.
