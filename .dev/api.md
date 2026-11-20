# Core service API

The core service is a Rust process on the local machine. The desktop UI and the phone shell talk to it. This document is the contract between them. Behavior comes from `.dev/SPEC.md`; this document names the messages.

The service binds `127.0.0.1` only. It accepts requests only for loopback hostnames and known desktop origins. In normal use, every action that starts a process comes from the UI. The API has no separate authentication; see the local-trust rule below.

## Request rules

- The `Host` header must name `127.0.0.1`, `localhost` or `[::1]`, with any port or no port. Any other host returns 403.
- If `Origin` is present, it must be one of `http://localhost:5173`, `http://127.0.0.1:5173`, `tauri://localhost`, `http://tauri.localhost` or `https://tauri.localhost`. Any other origin returns 403. Requests with no `Origin`, such as local scripts, are accepted.
- The service trusts the local user. Any program running as the same user can call the API, including the permission answer route. The Origin check stops web pages, not local programs. The permission card is a convenience, not a security boundary against local code.
- An allowed `Origin` is echoed in `Access-Control-Allow-Origin`, with `Vary: Origin`.
- An `OPTIONS` preflight returns 204 with `Access-Control-Allow-Methods: GET, POST, PATCH, DELETE, OPTIONS`, `Access-Control-Allow-Headers: content-type` and `Access-Control-Max-Age: 600`. A disallowed origin still returns 403.
- Every POST and PATCH request must send `Content-Type: application/json`. The media type may carry parameters such as `charset`. A missing or different content type returns 415.
- A project name must not be empty. Its folder must exist and be a directory. The folder is canonicalized, and two projects cannot use the same canonical folder.
- A project id is a unique slug of its name. A repeated name gets a numeric suffix.
- A project stores an optional default account and an optional default source branch. `PATCH /api/projects/{id}` sets a field by sending its value and clears it by sending `null`; an absent field keeps its current value. The default account must exist. A default branch must be a safe branch name and must exist as a local or remote branch in the project folder's Git repository; tags, other revisions and reflog expressions are rejected.
- A launch preset names an account and adds an optional model, environment overrides and config overrides. A preset with no stored record is derived from its account with the id `account:<account id>` and `stored: false`. Accounts with a stored preset do not get a second derived entry.
- Preset names are 1–64 characters. Up to 32 environment variables and 32 config keys are accepted. Config keys use letters, digits, `_`, `-` and dots; each dotted segment is non-empty and the key is at most 120 characters. Config values are at most 2000 characters.
- An environment variable or config key that names a secret, such as an API key, auth token, password or private key, returns 422. Keys belong in an account, not in a preset or a launch value.
- A preset model must fit the account's client. Antigravity uses lowercase slugs of letters, digits, `.`, `_` and `-`; other clients also accept uppercase letters and `:`, `/`, `@` and `+`. A model that does not fit returns 422. An unknown account returns 422. An invalid name, environment variable, config key or config value returns 400.
- `POST /api/presets` uses the account name when `name` is absent or empty. `PATCH` keeps a field when it is absent and replaces `env` or `config` when it is present. Sending `model: null` clears the model. An unknown stored preset returns 404.
- A session with `presetId` resolves a stored preset or an `account:<id>` entry. If `accountId` is absent, the preset's account is used; if both are present and differ, the request returns 422. An unknown preset returns 422. A terminal session with a preset returns 400.
- Preset environment variables apply first, then the session's `env` values. A per-launch value wins. The effective environment is stored on the session and applied again on reopen, account switch and restart. The model and config overrides are also applied again on restart.
- Config overrides apply to one launch. Claude Code writes them into its private settings file, Codex passes `-c key=value` arguments, OpenCode merges them into its launch config file, and Antigravity writes them into its private settings file. The model uses the same mechanism for each client. Values parse as JSON when possible, otherwise as text. Dotted keys create nested objects. Claude hooks and the OpenCode MCP and skill entries are added after the overrides and cannot be replaced. Nothing is written into the repository.
- Presets are saved in `state.json`. Account keys are never part of a preset.
- At most 64 sessions may be active at one time. This is a safety cap for memory and process limits. A new session above the cap returns 429. Closing a session frees its slot.
- At most 32 output streams and 32 event streams may be open at once. More returns 429.
- The optional `folder` is canonicalized. When it is absent, the project folder is used. A terminal session runs in that folder. For an agent, the folder must be inside a Git repository; the session runs in a new worktree under `<data dir>/worktrees/<session id>`.
- A session starts at 120 columns by 32 rows unless `cols` and `rows` are supplied. Column values must be 20–500 and row values 5–300; any other value returns 400.
- A live session's PTY can be resized. An unknown session returns 404 and a closed or exited session returns 409.
- `short` is unique within a project. A repeated name gets a numeric suffix. Agent branch names are also checked against existing Git refs, so a kept branch from an earlier session gets a new suffix.
- An agent session runs on branch `elesdee/<project>/<short>`. A terminal session has `branch: null`.
- An agent launch syncs the source before it creates the worktree. A remote source such as `origin/<branch>` fetches that remote and starts from its latest commit. A local source that tracks a remote branch fetches the remote and starts from the newer commit; when the local branch is behind and no worktree has it checked out, the branch is fast-forwarded. When the local and remote branches have diverged, the launch returns 422 and changes nothing. A folder with no remote, a branch with no upstream and a local-only branch start from the local commit without a fetch. A failed fetch or a Git timeout returns 422.
- `copyUncommittedChanges` copies the checkout's staged, unstaged and untracked changes into the new worktree. Tracked changes are applied as a patch against `HEAD`; untracked files are copied. The checkout is not changed. A patch that does not apply, a path outside the worktree, a special file, a non-UTF-8 path or more than 256 MiB of untracked files returns 422 and rolls the session back.
- `GET /api/git-status?folder=` reports whether the launch form should ask about uncommitted changes. It returns `{ "git": boolean, "dirty": boolean }`. A folder outside Git returns `git: false` with no error; an unknown folder returns 422.
- Closing an agent session stops the process, commits dirty files, pushes through the branch's upstream remote or the only remote, and removes the worktree on success. A failed commit, push, removal or Git timeout keeps the worktree and puts the reason in `preview`. Removal never uses `--force`. The branch is kept.
- While an agent session is closing, reopen and delete return 409 and `preview` is updated only when the close finishes.
- When a session process exits, the session stays `active`, its `workState` becomes `done`, and a `session.updated` event is sent. Input then returns 409.
- Renaming trims the name, requires 1–64 characters, and does not change `short` or `branch`.
- While a session runs, `preview` follows its latest non-empty output line. Terminal escape and control sequences are removed, whitespace is collapsed, and the text is capped at about 80 characters. It changes at most once every two seconds and sends `session.updated` only when the text changes. Closed sessions and close notes are never overwritten by output.
- Reopening requires a closed session. It re-checks the folder and, for an agent, the account, installed client, repository and branch. A missing agent worktree is recreated from the branch; a stale or unregistered path is replaced. It keeps the log and sequence numbers.
- Deleting requires a closed session and removes the session and its log. An agent branch is kept. If a failed close left a worktree, delete attempts to remove it.
- The client version check is cached for 10 seconds.

## List endpoints

- `GET /api/projects?q=` and `GET /api/accounts?q=` return a plain array for the pickers.
- `q` is optional and case-insensitive. A missing or empty value returns every item.
- Projects match on name or folder. Accounts match on name, client, provider or plan.
- Results are sorted by name, then id. Account responses never include a key. The Host and Origin rules apply.

## Accounts and Antigravity

- `POST /api/accounts` adds an API-key account. The body is `{ name, providerId, client, auth: "key", key, model? }`.
- This version accepts `providerId: "google"` with `client: "Antigravity"`. Any other provider and client pair, or another auth mode, returns 422. An empty name, an empty key or a model over 120 characters returns 400.
- The account id is a unique slug of the name. A repeated name gets a numeric suffix. The model defaults to `gemini-3.8-flash-medium`; model names use Antigravity's own slugs.
- The key is stored in the system keychain through the secret store. It is not written to `state.json`, returned by the API or written to logs. Account metadata is saved and returned in bootstrap.
- Seed accounts have `sample: true`. Accounts created through this endpoint have `sample: false`. Samples have no usage rows and exist only to show the interface.
- Antigravity runs `agy`. The core finds it on `PATH` and in common install paths.
- A launch reads the key, creates `<data dir>/homes/<session id>/.gemini/antigravity-cli/settings.json` with mode 0600 and `{"modelProvider":"gemini"}`, sets `HOME` to the session home and `GEMINI_API_KEY` to the key in the child environment, and runs the client in the worktree.
- The home is removed on close, agent exit, delete and shutdown. Reopen creates it again and reads the key again.
- Antigravity sessions get no Claude hook settings and no permission card. Their `shortcuts` list is empty; the UI shows the base keys.

## Usage sources

- Claude Code reports plan limits through a status line command that the core writes into the session's private settings file. The command posts the JSON that Claude Code sends on standard input to `/internal/statusline/claude/{id}` with the session's hook token. The core reads `rate_limits.five_hour` and `rate_limits.seven_day`, each with `used_percentage` and `resets_at`, for the account usage. It also reads `context_window.remaining_percentage` for the session's context-left value and `session_id` for the agent's conversation ID.
- Codex reports plan limits over the session's app-server websocket. The core sends `initialize`, `initialized` and `account/rateLimits/read`, then applies `account/rateLimits/updated` notifications. Windows are told apart by length: 300 minutes is the 5-hour window and 10,080 minutes is the weekly window.
- DeepSeek reports a prepaid balance from `GET https://api.deepseek.com/user/balance` with the account key from the keychain. The key is sent only in the request's `Authorization` header. It is never logged, returned or written to `state.json`.
- Other providers have no official source. The account keeps `foot: "No official usage source."`, has no usage status and is skipped by auto-switch.
- A reading stores the rows, the window kind, `usageSource`, `usageReadAt`, `usageCheckedAt` and `usageStatus`. `usageStatus` is `fresh` after a successful read, `stale` when the last read failed but earlier values remain, and `unknown` when no value was ever read. A failed read sends an `account.updated` event and leaves every session untouched.
- A fresh reading runs auto-switch immediately. A failed read never switches or pauses a session.
- DeepSeek balances are read in the background at most once a minute per account and only for non-sample accounts with a key. `POST /api/accounts/{id}/usage/refresh` reads at once, but two concurrent reads for the same account share one request, and a forced refresh within two seconds of the last attempt reuses the stored values.
- The manual route `POST /api/accounts/{id}/usage` remains for a caller that supplies rows itself. It sets `usageSource: "manual"` and a fresh status.

## Pairing and devices

The core starts one iroh endpoint for the phone link. The local routes below control pairing and the paired device list. They are local only. They are not reachable through the link, and they keep the Host and Origin rules.

```ts
type PairingState =
  | "idle"
  | "waiting"
  | "confirm"
  | "paired"
  | "denied"
  | "expired"
  | "cancelled";

interface PairingStatus {
  state: PairingState;
  expiresAt?: number;   // Unix seconds
  phoneName?: string;   // present in `confirm`
  code?: string;        // six digits, present in `confirm`
}

interface PairedDevice {
  endpointId: string;   // z-base-32 endpoint id
  name: string;
  platform: string;
  pairedAt: string;     // RFC 3339
  lastSeen?: string;    // RFC 3339, updated in memory at most once a minute
}
```

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| POST | `/api/pairing` | `{}` | `{ expiresAt, relayReady }` |
| GET | `/api/pairing/qr` | | `image/png` for the active ticket; 404 when idle |
| GET | `/api/pairing` | | `PairingStatus` |
| POST | `/api/pairing/approve` | `{}` | `PairedDevice` |
| POST | `/api/pairing/deny` | `{}` | `204` |
| DELETE | `/api/pairing` | | `204` |
| GET | `/api/devices` | | `PairedDevice[]` |
| DELETE | `/api/devices/{id}` | | `204` |

- A ticket lives for 120 seconds and is single use. A new request cancels the old ticket and any waiting confirmation.
- `POST /api/pairing` waits at most five seconds for a relay and sets `relayReady`. When the endpoint is not running, it returns 503 and pairing is unavailable. A debug build with `ELESDEE_DEV=1` also returns `ticket` for an emulator paste fallback.
- The QR route returns a PNG rendered in memory while the state is `waiting`. It returns 404 in every other state. The image and the ticket are never written to disk or a log.
- The desktop UI polls `GET /api/pairing` each second. In `confirm` it shows `phoneName` and the six-digit `code`. The phone shows the same code.
- Five wrong secrets cancel the session. A used, expired or cancelled ticket is rejected.
- `POST /api/pairing/approve` stores the device, saves state and returns 409 when no phone is waiting or the 16-device limit is reached. `deny` and `cancel` return 204 in every state.
- A device list entry holds no key and no secret. A 17th device returns 409.
- `DELETE /api/devices/{id}` returns 404 for an unknown id. It saves state and closes the device's live connections with link close code 3.
- The pairing handshake itself uses the pair ALPN, not these routes. It accepts one request and answers with a confirmation code, then a final `paired`, `denied` or `expired` message.

## Agent permissions

- Antigravity has no hooks and no permission card. Its own terminal handles prompts.
- A Claude Code agent starts with a private settings file under the data directory, mode 0600. The file carries a random per-session token, loopback hook URLs and a status line command that reports the context window to the same token. The token is never sent to the UI.
- The internal hook route needs the exact token. A wrong or missing token, a foreign Host, or a non-Claude session returns 403.
- A `PreToolUse` hook sets `pendingPermission` and `waitingFor: "permission"`, sends `session.updated`, and holds the hook response. The answer endpoint releases it. If another request is already held, the new hook gets `permissionDecision: "ask"` and the first request stays held.
- `summary` is a short single-line text taken from the command, path, query or URL, capped at 80 characters. Control, bidi and zero-width format characters are removed. Raw tool input is never stored in the session or written to logs.
- An answer resolves exactly one request. The fields stay set until the agent sends its next event, so a second answer gets 409. Two concurrent answers give one 204 and one 409.
- Closing the session or the agent exiting denies a pending request. A request with no answer expires after 10 minutes and is denied.
- The Claude settings file is removed on close, agent exit, delete and shutdown. A startup sweep removes stale files.
- `pendingPermission` and a `waitingFor` of `permission` are not written to `state.json`; the live session keeps them.
- `UserPromptSubmit` sets `working`. `PostToolUse` clears `waitingFor` and keeps the session active. `Notification` with a permission message sets `waitingFor: "permission"`. `MessageDisplay` and `Stop` update `preview`; `Stop` sets `done`.

## Session tools and skills

- Claude Code, Codex and OpenCode 2 sessions get the elesdee MCP server and the skill pack: `elesdee`, `ask-and-notify`, `coordinate-sessions`, `handoff` and `frontend-design`. Antigravity gets neither.
- The server exposes `ask_me`, `wait_for_answer`, `withdraw_question`, `notify_me`, `report_done`, `write_handoff_note`, `list_sessions`, `read_session`, `send_message`, `check_usage`, `start_session`, `wait_for_session` and `share_file`. Register demo server arrives with the demo servers.
- `wait_for_session` waits on another session's status and work state. `until` is `attention` (default: input, permission, question, pause, done or closed), `waiting`, `done`, `closed` or `change`. `timeoutSeconds` is 1–25. The call returns the session state and `timedOut`; it never blocks past the route timeout. It stops with an error when the calling session closes.
- `share_file` takes a path relative to the session folder and an optional caption. It copies the file into the shared store and returns the `SharedFile` record. A source that resolves outside the folder, a folder, a hard link, a secret file name, private-key content or a stored account key in the content is refused.
- The core writes each session's skill kit under `<data dir>/skills/<session id>`. Directories use mode 0700 and files use mode 0600. The kit is removed on close, agent exit, delete and shutdown, and a startup sweep removes stale kits.
- The kit holds `.claude-plugin/plugin.json` and `skills/<name>/SKILL.md`. Claude Code gets it as `--plugin-dir <kit>`.
- OpenCode 2 gets `<data dir>/mcp/opencode-<session id>.json`. Its `skills` array lists `<kit>/skills`, and `OPENCODE_CONFIG` points at the file. The file is removed with the session.
- Codex gets `CODEX_HOME=<kit>/codex` in its launch environment. Its skills are written under `<kit>/codex/skills`. When the user's Codex home has `auth.json` or `config.toml`, the launch links the file into the private home so the client sign-in and configuration still apply. The private home is removed with the kit.
- The launch does not write into the repository or change the user's global agent files.
- The `write_handoff_note` tool saves the handoff note on the session. The note is at most 8000 characters. A closed session refuses the tool. Saving the note emits `session.updated` and writes the state file.
- A switch to a different client sends the saved note and the end of the output log to the new agent as its first input. A switch inside the same client resumes the conversation and sends no context. The log tail is plain text, capped at 4000 characters; the whole context is capped at 6000 characters.
- A pause keeps the note. A later cross-client switch from the paused session sends it with the log tail.
- Reopening a closed agent session without a saved conversation ID sends the note and the log tail after the client starts.

## Switching accounts and context

- A switch needs an active agent session and a known account. A switch inside the same client resumes the client's conversation. A switch to a different client starts the new client with the saved handoff note and the end of the output log. A switch clears `contextLeft` because the new process reports its own figure later.
- `GET /api/sessions/{id}/switch-options` returns every account the switch endpoint accepts. The spec's per-session allowed-provider list is not stored yet, so the list is every account on the computer, including samples. It also returns `nextAccountId`: the account the automatic switch would pick under the current rule. That rule stays inside the session's client, skips the current account, samples and accounts without a known usage figure, and picks the account with the most usage left. The rule is narrower than the spec's subscription-first, cross-client order.
- A terminal session returns 422 for the options route. An unknown session returns 404. The options route does not check whether the session is closed; the UI offers no switch for a closed session.
- `contextLeft` is the percentage of the client's context window that is still free. For Claude Code it comes from the status line JSON, field `context_window.remaining_percentage`, rounded and clamped to 0–100. Other clients report no figure yet, so the value stays `null`.
- The Claude settings file adds a `statusLine` command that sends the JSON to `POST /internal/statusline/claude/{id}?token=...`. The route uses the same per-session token as the hooks. A wrong token or a non-Claude session returns 403. The route accepts `{ session_id?, context_window? }`, updates the session only when the rounded value changes, sends `session.updated`, and returns 204.
- A missing or null `remaining_percentage` sets `contextLeft` to `null`. A switch, resume or restart recovery clears the value. The value is saved with the session but the client replaces it after it starts.

## Persistence

- State lives in `ELESDEE_DATA_DIR`, default `~/.elesdee`, in `state.json`. The directory is created with mode 0700 and files with mode 0600. Writes use a temporary file, fsync and rename.
- Projects and their default account and source branch, accounts, launch presets, sessions, each session's desired columns and rows, its output log, an agent's repository root, an agent's own conversation ID, the session's handoff note and last context-left figure, and the paired device list are saved. Account keys and pairing secrets are not saved. No runtime handles are saved.
- The desktop link identity is a separate 32-byte key in `<data dir>/link.key`, with mode 0600, written once and reused. It is never in `state.json` and never logged. A missing key creates a new identity; every paired phone must pair again.
- The paired device list is written on pair, removal and graceful shutdown. `lastSeen` updates in memory at most once a minute.
- Shared files live in `<data dir>/shared/<session id>/`, directories 0700 and files 0600. Each copy has a metadata record beside it with the original relative path, name, caption, size and content type. The stored copy stays after the session closes. Account keys are compared with the content and are never copied.
- State is saved on project create, account create, session create, rename, close, reopen, delete, resize, graceful shutdown, and when an agent reports its conversation ID. It is not written for each output chunk.
- A session that was active on save is restarted when the state loads. It resumes the agent's conversation by its saved `agentSessionId`; without an ID it uses the client's continue-last behavior and the preview says so. A terminal session restarts its shell in the same folder. The log and sequence numbers are kept, so new output continues from the next sequence.
- If the session folder, account or client is missing, that session is restored as active and paused with a clear reason. The other sessions still restart. A pending permission request is not written to the file and is not restored.
- `agentSessionId` comes from the client. Claude Code gets a generated `--session-id` at launch and also reports it in hook input. Codex reports the app-server thread ID. OpenCode reports the session ID on its local server. Antigravity has no ID and uses its continue-last behavior.
- A new state starts with no projects. Seed accounts are used only when no state file exists, when the state file is corrupt, or when an older state file has no account list. A corrupt file is moved to `state.bad.json` and the problem is logged.

## Types

```ts
type SessionState = "active" | "closed";
type WorkState = "working" | "waiting" | "input" | "paused" | "done";
type WaitingFor = "permission" | "input" | "start" | "question";

type SessionKind = "agent" | "terminal";
type Client = "Claude Code" | "Codex" | "OpenCode 2" | "Antigravity";

interface PendingPermission {
  requestId: string;
  tool: string;
  summary: string;
}

interface Shortcut {
  key: string;          // "Shift+Tab"
  label: string;        // "⇧Tab"
  name: string;         // "Shift Tab", for screen readers
  description: string;  // "Cycle permission modes"
}

interface Session {
  id: string;
  name: string;
  short: string;
  projectId: string;
  kind: SessionKind;
  status: SessionState;
  workState: WorkState | null;   // detail inside active
  client: Client | null;
  accountId: string | null;
  agentSessionId: string | null; // the client's own conversation ID
  handoffNote?: string | null;   // kept for the next agent that works on the session
  contextLeft: number | null;    // percent of the context window free, or null when unknown
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  envOverrides: EnvOverride[];   // the effective launch environment
  presetId: string | null;
  model: string | null;
  config: Record<string, string>; // config overrides applied at launch
  firstPrompt?: string | null;
  parentSessionId?: string | null;
  parentSessionName?: string | null;
  startedAt: string;             // RFC 3339
  shortcuts: Shortcut[];         // supplied by the server for this client
}

interface EnvOverride {
  name: string;
  value: string;
}

interface Preset {
  id: string;                    // unique slug of the name
  name: string;
  accountId: string;
  model: string | null;
  env: EnvOverride[];
  config: Record<string, string>; // keys may use dots for nested settings
  createdAt: string;
  updatedAt: string;
}

interface LaunchPreset extends Omit<Preset, 'createdAt' | 'updatedAt'> {
  id: string;                    // a stored preset id, or "account:<account id>"
  stored: boolean;
  createdAt: string | null;      // null for an account-derived preset
  updatedAt: string | null;
}

interface SharedFile {
  id: string;
  sessionId: string;
  sessionName: string;
  name: string;          // the original file name
  path: string;          // relative to the session folder
  caption: string | null;
  size: number;
  contentType: string;
  image: boolean;
  createdAt: string;     // RFC 3339
}

interface Project {
  id: string;
  name: string;
  folder: string;
  defaultAccountId: string | null;
  defaultSourceBranch: string | null;
}

interface SwitchOptions {
  accounts: Account[];           // every account the switch endpoint accepts
  nextAccountId: string | null;  // the account auto-switch would pick next
}

interface FileEntry {
  name: string;
  path: string;       // relative to the session folder, "/" separated
  directory: boolean;
}

interface FileList {
  path: string;       // the listed folder, "" for the root
  entries: FileEntry[];
  truncated: boolean; // true when the folder has more entries than the cap
}

interface FileContent {
  path: string;
  content: string;    // UTF-8 text
  size: number;       // bytes
}

type ChangeStatus =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "untracked"
  | "conflicted";

interface ChangedFile {
  path: string;
  oldPath: string | null;   // set for a rename or copy
  status: ChangeStatus;
  additions: number;
  deletions: number;
  binary: boolean;
  diff: string;             // unified diff, empty for binary files
  diffTruncated: boolean;
}

interface Changes {
  git: boolean;             // false for a folder outside Git
  base: string | null;      // the comparison commit, null for a plain folder
  files: ChangedFile[];
  truncated: boolean;       // the file or diff cap was reached
  error: string | null;     // a Git failure the UI can show
}

type UsageStatus = "fresh" | "stale" | "unknown";

interface UsageRow {
  label: string;                 // fallback text when the window is unknown
  window?: "five_hour" | "seven_day" | "balance" | string;
  used?: number;                 // percent used
  value?: string;                // text value, such as a balance
  min?: string;
  warn?: boolean;
  resetsAt?: number;             // Unix seconds
}

interface Account {
  id: string;
  name: string;
  plan: string;
  client: Client;
  provider: string;
  auth: "subscription" | "key" | "local";
  sample: boolean;              // seed account, not added by the user
  rows: UsageRow[];
  foot: string;                 // "No official usage source." when none exists
  usageReadAt: string | null;   // RFC 3339, last successful read
  usageCheckedAt: string | null; // RFC 3339, last attempt
  usageStatus: UsageStatus | null;
  usageSource: "statusline" | "app-server" | "deepseek" | "manual" | null;
  server: string | null;
  model: string | null;
}

interface AgentCheck {
  name: Client;
  installed: boolean;
  version: string | null;
}

interface OutputGap {
  after: number;        // sequence the client asked for
  oldestSeq: number;    // oldest sequence still retained
  newestSeq: number;    // newest sequence at the time of the request
}

interface OutputEvent {
  seq: number;
  data: string;
  gap?: OutputGap;      // only on the first replayed event when old output was dropped
}
```

## Endpoints

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| GET | `/api/health` | | `{ ok, version, agents: AgentCheck[] }` |
| GET | `/api/bootstrap` | | `{ projects, accounts, presets, sessions }` |
| GET | `/api/projects?q={text}` | | `Project[]` |
| POST | `/api/projects` | `{ name, folder }` | `Project` |
| PATCH | `/api/projects/{id}` | `{ defaultAccountId?, defaultSourceBranch? }` | `Project` |
| GET | `/api/git-status?folder={folder}` | | `{ git, dirty }` |
| GET | `/api/accounts?q={text}` | | `Account[]` |
| POST | `/api/accounts` | `{ name, providerId, client, auth: "key", key, model? }` | `Account` |
| POST | `/api/accounts/{id}/usage` | `{ rows: UsageRow[] }` | `Account` |
| POST | `/api/accounts/{id}/usage/refresh` | `{}` | `Account`; 502 when the read fails |
| POST | `/internal/statusline/claude/{id}?token={token}` | the Claude Code status line JSON | `204` |
| POST | `/api/pairing` | `{}` | `{ expiresAt, relayReady }` |
| GET | `/api/pairing/qr` | | `image/png` |
| GET | `/api/pairing` | | `PairingStatus` |
| POST | `/api/pairing/approve` | `{}` | `PairedDevice` |
| POST | `/api/pairing/deny` | `{}` | `204` |
| DELETE | `/api/pairing` | | `204` |
| GET | `/api/devices` | | `PairedDevice[]` |
| DELETE | `/api/devices/{id}` | | `204` |
| GET | `/api/presets?q={text}` | | `LaunchPreset[]` |
| POST | `/api/presets` | `{ name?, accountId, model?, env?, config? }` | `LaunchPreset` |
| PATCH | `/api/presets/{id}` | `{ name?, accountId?, model?, env?, config? }` | `LaunchPreset` |
| DELETE | `/api/presets/{id}` | | `204` |
| POST | `/api/sessions` | `{ projectId, kind, accountId?, presetId?, name?, folder?, sourceBranch?, copyUncommittedChanges?, env?, cols?, rows? }` | `Session` |
| POST | `/api/sessions/{id}/close` | `{}` | `Session` |
| PATCH | `/api/sessions/{id}` | `{ name }` | `Session` |
| POST | `/api/sessions/{id}/reopen` | `{}` | `Session` |
| POST | `/api/sessions/{id}/account` | `{ accountId }` | `Session` |
| GET | `/api/sessions/{id}/switch-options` | | `SwitchOptions` |
| DELETE | `/api/sessions/{id}` | | `204` |
| POST | `/api/sessions/{id}/input` | `{ data }` (UTF-8 text) | `204` |
| POST | `/api/sessions/{id}/resize` | `{ cols, rows }` | `204` |
| POST | `/api/sessions/{id}/permission` | `{ requestId, decision: "allow" \| "deny" }` | `204` |
| GET | `/api/sessions/{id}/files?path={folder}` | | `FileList` |
| GET | `/api/sessions/{id}/file?path={file}` | | `FileContent` |
| POST | `/api/sessions/{id}/file` | `{ path, content }` | `FileContent` |
| GET | `/api/sessions/{id}/changes` | | `Changes` |
| GET | `/api/sessions/{id}/shared` | | `SharedFile[]` |
| GET | `/api/sessions/{id}/shared/{fileId}` | | file bytes; `Content-Type` from the file |
| GET | `/api/sessions/{id}/output?after={seq}` | | SSE: `data: {"seq":n,"data":"..."}` and an optional `gap` |
| GET | `/api/events` | | SSE: `data: {"type":"session.updated","session":Session}` |

Errors use `{ "error": "message" }`. Pairing returns 503 when the link endpoint is not running, and 409 when no phone waits for approval or the device limit is reached. Removing an unknown device returns 404. Unknown sessions return 404. A session that is closed, or whose process has exited, returns 409 for input. An invalid session name or terminal size returns 400. An agent session in a non-Git folder returns 422. Reopening an active session, deleting an active session, or reopening or deleting while a close is running returns 409; reopening with a missing folder, repository, branch or client returns 422. A permission answer returns 404 for an unknown session and 409 when the session is closed, there is no pending request, the request id does not match, or the request was already answered. An empty project name returns 400, a bad project folder returns 422, and a duplicate project folder returns 409. An unknown project returns 404. A project change that cannot be written to disk returns 500 and is rolled back in memory. A default account that does not exist, a default branch that does not exist, or a project folder that is not a Git repository returns 422. An unsafe branch name returns 400. An empty account name, empty key or overlong model returns 400; an unsupported provider and client pair or an unsupported auth mode returns 422; a keychain failure returns 500. An unknown preset account or a model that does not fit the client returns 422; an invalid preset name, environment variable or config key returns 400; a secret-like environment variable or config key returns 422; an unknown stored preset returns 404. A blocked Host or Origin returns 403, a bad POST or PATCH content type returns 415, a full session list or stream list returns 429, a request that exceeds the 30-second timeout returns 408, and a wrong non-OPTIONS method returns 405. A file path outside the session folder returns 400, a binary file returns 422, and content above 1 MiB returns 413.

## Rules

- A terminal session runs a real shell in its folder through a PTY. Closing the session stops the process group and keeps the folder.
- Close commits leftover work on the session branch and pushes it. When the session has a handoff note, the commit message body carries the note. A clean tree keeps its last commit; if that commit does not carry the note, Close adds an empty commit that does. The note is never written as a file.
- The file and change routes use the session folder: the worktree for an agent, the chosen folder for a terminal. A missing folder returns 404.
- A file path is relative to the session folder and is canonicalized. An absolute path, a `..` component and a symlink that resolves outside the folder return 400. A folder where a file is expected returns 422. A path with a `.git` component is refused, as is a hard-linked file and a named pipe or other special file. The listing skips `.git` folders, symlinks and names that are not UTF-8; a skipped name marks the listing truncated.
- A shared file is copied from the session folder, never linked to the source. The store holds at most 10 MiB per file. The list is newest first. The read route returns 404 for an unknown id, 413 above the cap and 500 when the store is unavailable. Images are served inline; other files are served as attachments with a sanitized name. The response is not cached.
- One folder listing returns at most 2000 entries and sets `truncated` when more exist. A file read or write is UTF-8 text of at most 1 MiB. A larger file or request body returns 413. Binary content, meaning a NUL byte in the first 8 KiB, returns 422.
- `POST /api/sessions/{id}/file` creates the file or replaces its content. The parent folder must exist. The write is not limited to existing files. A write goes to a temporary file in the same folder and is renamed into place, so a failure cannot leave a half-written file. An existing file keeps its permission bits.
- Changes for an agent session compare the worktree, including uncommitted work, against the merge base with the repository's current branch. Changes for a terminal session compare the working tree against `HEAD`. Untracked files are included.
- The change route caps the list at 500 files and the combined diffs at 512 KiB. It sets `truncated` when either cap is reached. A folder outside Git returns `git: false` with no files and no error.
- An agent session runs the chosen client when one is installed. The client command is chosen by the server, never sent by the UI.
- The server tells the UI the client's key shortcuts. The UI keeps no list of client keys. An Antigravity session gets an empty list, so the UI shows the base keys.
- Every change to a session sends `session.updated` on `/api/events`.
- Every usage reading, successful or failed, sends `account.updated` on `/api/questions/events` with the full account. The desktop UI replaces that account in its list; no other account field changes.
- Output events carry a sequence number. `after=n` replays the output with a higher sequence number.
- `after` must be at most the newest sequence. A greater value returns 400.
- The retained log is bounded. When old chunks have been dropped, the first replayed event carries `gap` with the oldest retained and newest known sequence, so the UI can show that text was lost.
- Both SSE endpoints send an initial comment and a comment every 15 seconds while idle. An output stream ends when its session closes.
- The output log is bounded per session. Closed session logs share a bounded memory budget; the oldest closed logs are dropped first.
