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
- At most 64 sessions may be active at one time. A new session above the cap returns 429. Closing a session frees its slot.
- At most 32 output streams and 32 event streams may be open at once. More returns 429.
- The optional `folder` is canonicalized. When it is absent, the project folder is used. A terminal session runs in that folder. For an agent, the folder must be inside a Git repository; the session runs in a new worktree under `<data dir>/worktrees/<session id>`.
- A session starts at 120 columns by 32 rows unless `cols` and `rows` are supplied. Column values must be 20–500 and row values 5–300; any other value returns 400.
- A live session's PTY can be resized. An unknown session returns 404 and a closed or exited session returns 409.
- `short` is unique within a project. A repeated name gets a numeric suffix. Agent branch names are also checked against existing Git refs, so a kept branch from an earlier session gets a new suffix.
- An agent session runs on branch `elesdee/<project>/<short>`. A terminal session has `branch: null`.
- Closing an agent session stops the process, commits dirty files, pushes through the branch's upstream remote or the only remote, and removes the worktree on success. A failed commit, push, removal or Git timeout keeps the worktree and puts the reason in `preview`. Removal never uses `--force`. The branch is kept.
- While an agent session is closing, reopen and delete return 409 and `preview` is updated only when the close finishes.
- When a session process exits, the session stays `active`, its `workState` becomes `done`, and a `session.updated` event is sent. Input then returns 409.
- Renaming trims the name, requires 1–64 characters, and does not change `short` or `branch`.
- While a session runs, `preview` follows its latest non-empty output line. Terminal escape and control sequences are removed, whitespace is collapsed, and the text is capped at about 80 characters. It changes at most once every two seconds and sends `session.updated` only when the text changes. Closed sessions and close notes are never overwritten by output.
- Reopening requires a closed session. It re-checks the folder and, for an agent, the account, installed client, repository and branch. A missing agent worktree is recreated from the branch; a stale or unregistered path is replaced. It keeps the log and sequence numbers.
- Deleting requires a closed session and removes the session and its log. An agent branch is kept. If a failed close left a worktree, delete attempts to remove it.
- The client version check is cached for 10 seconds.

## Accounts and Antigravity

- `POST /api/accounts` adds an API-key account. The body is `{ name, providerId, client, auth: "key", key, model? }`.
- This version accepts `providerId: "google"` with `client: "Antigravity"`. Any other provider and client pair, or another auth mode, returns 422. An empty name, an empty key or a model over 120 characters returns 400.
- The account id is a unique slug of the name. A repeated name gets a numeric suffix. The model defaults to `gemini-3.8-flash-medium`; model names use Antigravity's own slugs.
- The key is stored in the system keychain through the secret store. It is not written to `state.json`, returned by the API or written to logs. Account metadata is saved and returned in bootstrap.
- Antigravity runs `agy`. The core finds it on `PATH` and in common install paths.
- A launch reads the key, creates `<data dir>/homes/<session id>/.gemini/antigravity-cli/settings.json` with mode 0600 and `{"modelProvider":"gemini"}`, sets `HOME` to the session home and `GEMINI_API_KEY` to the key in the child environment, and runs the client in the worktree.
- The home is removed on close, agent exit, delete and shutdown. Reopen creates it again and reads the key again.
- Antigravity sessions get no Claude hook settings and no permission card. Their `shortcuts` list is empty; the UI shows the base keys.

## Agent permissions

- Antigravity has no hooks and no permission card. Its own terminal handles prompts.
- A Claude Code agent starts with a private settings file under the data directory, mode 0600. The file carries a random per-session token and loopback hook URLs. The token is never sent to the UI.
- The internal hook route needs the exact token. A wrong or missing token, a foreign Host, or a non-Claude session returns 403.
- A `PreToolUse` hook sets `pendingPermission` and `waitingFor: "permission"`, sends `session.updated`, and holds the hook response. The answer endpoint releases it. If another request is already held, the new hook gets `permissionDecision: "ask"` and the first request stays held.
- `summary` is a short single-line text taken from the command, path, query or URL, capped at 80 characters. Control, bidi and zero-width format characters are removed. Raw tool input is never stored in the session or written to logs.
- An answer resolves exactly one request. The fields stay set until the agent sends its next event, so a second answer gets 409. Two concurrent answers give one 204 and one 409.
- Closing the session or the agent exiting denies a pending request. A request with no answer expires after 10 minutes and is denied.
- The Claude settings file is removed on close, agent exit, delete and shutdown. A startup sweep removes stale files.
- `pendingPermission` and a `waitingFor` of `permission` are not written to `state.json`; the live session keeps them.
- `UserPromptSubmit` sets `working`. `PostToolUse` clears `waitingFor` and keeps the session active. `Notification` with a permission message sets `waitingFor: "permission"`. `MessageDisplay` and `Stop` update `preview`; `Stop` sets `done`.

## Persistence

- State lives in `ELESDEE_DATA_DIR`, default `~/.elesdee`, in `state.json`. The directory is created with mode 0700 and files with mode 0600. Writes use a temporary file, fsync and rename.
- Projects, accounts, sessions, each session's desired columns and rows, its output log, and an agent's repository root are saved. Account keys are not saved. No runtime handles are saved.
- State is saved on project create, account create, session create, rename, close, reopen, delete, resize, and graceful shutdown. It is not written for each output chunk.
- A session that was active on save is restored as closed, with preview `"The app restarted. Reopen to continue."` and no process. Its log and sequence numbers are kept, and reopen continues from the next sequence. A pending permission request is not written to the file and is not restored.
- Seed projects and seed accounts are used only when no state file exists, when the state file is corrupt, or when an older state file has no account list. A corrupt file is moved to `state.bad.json` and the problem is logged.

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
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  startedAt: string;             // RFC 3339
  shortcuts: Shortcut[];         // supplied by the server for this client
}

interface Project {
  id: string;
  name: string;
  folder: string;
}

interface UsageRow {
  label: string;
  used?: number;                 // percent used
  value?: string;                // text value, such as a balance
  min?: string;
  warn?: boolean;
}

interface Account {
  id: string;
  name: string;
  plan: string;
  client: Client;
  provider: string;
  auth: "subscription" | "key" | "local";
  rows: UsageRow[];
  foot: string;
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
| GET | `/api/bootstrap` | | `{ projects, accounts, sessions }` |
| POST | `/api/projects` | `{ name, folder }` | `Project` |
| POST | `/api/accounts` | `{ name, providerId, client, auth: "key", key, model? }` | `Account` |
| POST | `/api/sessions` | `{ projectId, kind, accountId?, name?, folder?, cols?, rows? }` | `Session` |
| POST | `/api/sessions/{id}/close` | `{}` | `Session` |
| PATCH | `/api/sessions/{id}` | `{ name }` | `Session` |
| POST | `/api/sessions/{id}/reopen` | `{}` | `Session` |
| DELETE | `/api/sessions/{id}` | | `204` |
| POST | `/api/sessions/{id}/input` | `{ data }` (UTF-8 text) | `204` |
| POST | `/api/sessions/{id}/resize` | `{ cols, rows }` | `204` |
| POST | `/api/sessions/{id}/permission` | `{ requestId, decision: "allow" \| "deny" }` | `204` |
| GET | `/api/sessions/{id}/output?after={seq}` | | SSE: `data: {"seq":n,"data":"..."}` and an optional `gap` |
| GET | `/api/events` | | SSE: `data: {"type":"session.updated","session":Session}` |

Errors use `{ "error": "message" }`. Unknown sessions return 404. A session that is closed, or whose process has exited, returns 409 for input. An invalid session name or terminal size returns 400. An agent session in a non-Git folder returns 422. Reopening an active session, deleting an active session, or reopening or deleting while a close is running returns 409; reopening with a missing folder, repository, branch or client returns 422. A permission answer returns 404 for an unknown session and 409 when the session is closed, there is no pending request, the request id does not match, or the request was already answered. An empty project name returns 400, a bad project folder returns 422, and a duplicate project folder returns 409. An empty account name, empty key or overlong model returns 400; an unsupported provider and client pair or an unsupported auth mode returns 422; a keychain failure returns 500. A blocked Host or Origin returns 403, a bad POST or PATCH content type returns 415, a full session list or stream list returns 429, a request that exceeds the 30-second timeout returns 408, and a wrong non-OPTIONS method returns 405.

## Rules

- A terminal session runs a real shell in its folder through a PTY. Closing the session stops the process group and keeps the folder.
- An agent session runs the chosen client when one is installed. The client command is chosen by the server, never sent by the UI.
- The server tells the UI the client's key shortcuts. The UI keeps no list of client keys. An Antigravity session gets an empty list, so the UI shows the base keys.
- Every change to a session sends `session.updated` on `/api/events`.
- Output events carry a sequence number. `after=n` replays the output with a higher sequence number.
- `after` must be at most the newest sequence. A greater value returns 400.
- The retained log is bounded. When old chunks have been dropped, the first replayed event carries `gap` with the oldest retained and newest known sequence, so the UI can show that text was lost.
- Both SSE endpoints send an initial comment and a comment every 15 seconds while idle. An output stream ends when its session closes.
- The output log is bounded per session. Closed session logs share a bounded memory budget; the oldest closed logs are dropped first.
