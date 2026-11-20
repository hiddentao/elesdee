# Merge review 2: phone, browser, launch, updates

Range: `8ec30a1..8f24897` on `hiddentao/round-2-mockups`. This covers the merges for phone files and uploads, browser live input and logs, close-to-background, prepaid minimum balance, launch grouping and recent repositories, phone notifications, the phone UI package and bridge transport, session drag, client updates, and the spec-review fixes (waiting input, image size, console redaction, modified browser keys, launch GitHub search, source repository).

Method: code reading against `.dev/SPEC.md`, `.dev/api.md`, `.dev/phone-shell.md` and `.dev/browser.md`, the full test suites, and live tests against the built core service and `git`. No code was edited.

Tests run: `cargo test` in `core/` passes (195 unit, 211 API, 8 GitHub, 16 link). `cargo test` in `link/` passes. `npm run test` at the root passes (286 app tests). `npm --prefix phone-shell run test` passes (106 tests). Both typecheck commands pass.

## Findings

### 1. Medium: a GitHub repository name can be parsed as a `git clone` option

- Where: `core/src/github.rs:968-980` (`run_clone`) runs `git clone --quiet <url> <target>` with no `--` separator. The target is `parent.join(&repo.name)` at `core/src/github.rs:876`, where `repo.name` comes from the GitHub API. `valid_folder_name` (`core/src/github.rs:1020-1028`) rejects separators and control characters but accepts a leading hyphen.
- Scenario (confirmed locally with git): a repository named `--no-checkout` is offered in the launch picker. Git parses the target argument as an option, clones into the default destination (which happens to be the same folder name) and checks out nothing. The clone succeeds, `clone_repo` adds the project, and the agent session starts in a folder that contains only `.git`. A repository named `--mirror` or `--bare` makes git clone to `<name>.git` instead: the app then fails to canonicalize the target, returns an error, and leaves the stray clone in the user's parent folder. `--recurse-submodules` and similar flags also change behavior. The repo list comes from `/user/repos?affiliation=owner,collaborator,organization_member`, so its names are not under the app's control.
- Fix: add `--` before the URL and target in `run_clone` (`git clone --quiet -- <url> <target>`) and reject a repository name that begins with `-` in `valid_folder_name`. Add a unit test with a `--no-checkout` target.

### 2. Medium: the launch picker can start a session in the wrong repository

- Where: `app/src/components/NewSessionDialog.tsx:142-168` (`githubRepoLocalFolder`) maps a GitHub repository marked `added` to a local folder by comparing folder **basenames** with the repository name. The core marks `added` by comparing normalized git remotes (`core/src/github.rs:786-808`), so the core knows the exact folder but does not return it.
- Scenario: project `/dev/web` is a clone of `github.com/me/web`. The user also cloned `github.com/acme/web` to `/work/acme-web`. The API marks `acme/web` as `added` because the acme remote exists. In the launch form `githubRepoLocalFolder` finds exactly one project whose basename is `web`, `/dev/web`, and offers the `acme/web` entry with the id `/dev/web`. Selecting it starts the session in the wrong repository. In the reverse case, where the added copy has a different folder basename, the picker finds no local folder and offers a fresh clone, so the user can create a duplicate checkout.
- Fix: return the matched project folder (or project id) from the core with each repository, using the same `added_remotes` scan, and use that value in the picker instead of a basename guess.

### 3. Medium: a phone UI package update breaks the package that is already open

- Where: `phone-shell/src-tauri/src/ui.rs:462-486` serves only the build that is currently `active`; `commands.rs:1229-1232` runs `sync_ui` on every reconnect, and `UiStore::use_build` (`phone-shell/src-tauri/src/ui.rs:308-319`) makes the newly downloaded build active.
- Scenario: the phone is using package build X (the webview URL is `/ui/blake3-<X>/`). The computer updates its package and the link reconnects. The supervisor downloads build Y and sets it active. The open webview still requests assets and chunk files under build X; `handle` rejects every path whose build is not the active one, so those requests get 404. A page reload or a lazily loaded route then fails, and the shell has no way to navigate the package to Y because the shell screens were replaced by the package.
- Fix: serve any installed, hash-verified build for that computer (the build id is a content hash and `read_package_file` still confines paths), or send the new `phone:ui` status to the open package and have the package navigate to the new URL. Keep the current build and one previous build, which the store already does.

### 4. Low: a cleared provider minimum balance comes back on the next unrelated PATCH

- Where: `core/src/api.rs:1479-1488`. For an absent `minimumBalance`, the update uses `existing.minimum_balance.or(Some(DEFAULT_MINIMUM_BALANCE))`. `.dev/api.md:155` says an absent field keeps its value and "A cleared minimum stays cleared".
- Scenario (confirmed against the running service): create a DeepSeek provider (minimum defaults to 5). PATCH `{"minimumBalance": null}` returns `null`. PATCH `{"name": "Renamed"}` with no minimum returns `minimumBalance: 5.0`. The user's cleared floor silently returns, so auto-switch treats a balance at or below 5 as no room again.
- Fix: use `existing.minimum_balance` for an absent field. Apply the default only on create, or migrate providers that predate the field once at load (as the source-repository migration does), so "never set" and "cleared" stay distinct.

### 5. Low: uploaded phone files survive session deletion

- Where: `core/src/api.rs:3782-3829` (`delete_session`) removes the worktree, session files, Claude settings and the Antigravity home, but not `<data dir>/uploads/<session id>` written by `core/src/files.rs:403-413`. No startup sweep removes it either.
- Scenario (confirmed against the running service): upload `note.txt` to a terminal session, close it and delete it. The delete returns 204 and the session disappears, but `<data>/uploads/<id>/note.txt` remains. Uploaded files can be up to 25 MiB each, so deleted sessions leave private files and unbounded disk use behind.
- Fix: remove `<data>/uploads/<session id>` when a session is deleted, and sweep upload folders whose session id is not in the loaded state at startup.

## Checked and correct

- Phone file reads and images resolve under the canonical session folder, refuse `..`, absolute paths, `.git` paths, symlinks, hard links, special files, binary text and oversized files. Uploads are cleaned to one path component, never overwrite, use private modes and are removed again when the session cannot accept the input.
- The link method allowlist covers every dispatched method, unknown names are rejected during frame decoding, and upload and image bodies have explicit caps. Streams are bounded per connection and in total.
- The phone UI package is checked by size, entry count, entry names, symlink refusal, case-insensitive duplicate names, an index requirement and the BLAKE3 hash before it is unpacked; the extraction creates new files only. The zip crate enforces the declared uncompressed size while reading, so a lying header fails instead of expanding past the cap.
- The browser install uses a pinned version and SHA-256 plus the published checksum, unpacks with traversal and symlink-target checks, and refuses oversized archives. Browser input stays inside the session's page, named keys and modifier masks are validated, and live input points are clamped.
- Browser console and network logs are bounded and cleaned; addresses lose login details and sensitive query values, and the agent tools remove the session and provider secrets before returning text.
- Close-to-background hides the window on macOS and keeps the core, sessions and phone link alive. Quit still asks, sets a confirmed flag, stops the core with SIGTERM and waits before forcing it.
- The prepaid minimum compares the parsed balance with the provider floor, counts a balance at or below the floor as no room, and skips a balance with no floor.
- Session move changes only `projectId`; open tabs follow, and the source repository, worktree, branch and log are kept.
- Client install and update stage into a private folder, run the new copy's `--version`, verify the binary is inside the staged folder, rewrite absolute links, and swap with a rollback when the rename fails. A user or bundled copy is never modified.
- Waiting-input and question states follow `.dev/hooks.md`: idle signals set input, permission and question states are preserved, and a pending question is not cleared by a running turn. Reopen and recovery paths are unchanged.
