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
- A project stores an optional default AI config and an optional default source branch. `PATCH /api/projects/{id}` sets a field by sending its value and clears it by sending `null`; an absent field keeps its current value. The default config must exist. A default branch must be a safe branch name and must exist as a local or remote branch in the project folder's Git repository; tags, other revisions and reflog expressions are rejected.
- A provider holds a credential and an endpoint. Its id is a unique slug of its name. The name is 1–80 characters. The vendor comes from the provider catalog. The auth method must be one the vendor offers. A custom provider needs an API style (`openai` or `anthropic`) and a server URL. A local auth and the Ollama and LM Studio vendors need a server URL. Other vendors take an optional server override.
- An AI config names a provider and one client that the provider supports. Its id is a unique slug of its name. The name is 1–64 characters and defaults to `<provider> (<client>)`. The model is optional and at most 120 characters; Antigravity accepts lowercase slugs of letters, digits, `.`, `_` and `-`, and other clients also accept `:`, `/`, `@` and `+`. The effort must be one of the levels the client offers. Up to 64 extra arguments are accepted, each at most 2000 characters with no NUL. Up to 32 environment variables and 32 config keys are accepted. Config keys use letters, digits, `_`, `-` and dots; each dotted segment is non-empty and the key is at most 120 characters. Config values are at most 2000 characters.
- An environment variable or config key that names a secret, such as an API key, auth token, password or private key, returns 422. Keys belong in a provider, not in a config or a launch value.
- A session with `configId` resolves a stored config. The older `accountId` or a `providerId` resolves that provider's first config, so an older client or the phone keeps working. An unknown config returns 404. A terminal session with a config returns 400.
- An agent launch may send `allowedConfigIds`. An absent list allows every non-sample config of the starting client plus the starting config. A terminal session with a list returns 400. The list must hold existing configs, has no duplicates, and includes the starting config.
- A config's environment variables apply first, then the session's `env` values. A per-launch value wins. The config's environment is applied again on reopen and switch. The model, effort and config overrides are applied again on restart.
- Config overrides apply to one launch. Claude Code writes them into its private settings file, Codex passes `-c key=value` arguments, OpenCode merges them into its launch config file, and Antigravity writes them into its private settings file. Values parse as JSON when possible, otherwise as text. Dotted keys create nested objects. Claude hooks and the OpenCode MCP and skill entries are added after the overrides and cannot be replaced. Nothing is written into the repository.
- Model and effort use the client's own form. Claude Code gets `--model <model>` and `--effort <level>`. Codex gets `-c model="<model>"` and `-c model_reasoning_effort="<level>"`. OpenCode 2 gets `model` and `reasoningEffort` in its generated config. Antigravity gets `model` and `effort` in its private settings file. Extra command-line arguments are appended after the generated model, effort and tool arguments.
- Configs and providers are saved in `state.json`. Keys are never part of either record.
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
- Reopening requires a closed session. It re-checks the folder and, for an agent, the AI config, installed client, repository and branch. A missing agent worktree is recreated from the branch; a stale or unregistered path is replaced. It keeps the log and sequence numbers.
- Deleting requires a closed session and removes the session and its log. An agent branch is kept. If a failed close left a worktree, delete attempts to remove it.
- The client version check is cached for 10 seconds.

## Client discovery and install

- `GET /api/health` returns one `AgentCheck` for each of the four clients. `path` is the executable that the check runs. The client search checks `PATH`, the user install folders, the Antigravity folders, Homebrew, the app bundle and the app's own copy in its data folder, and it never runs a login shell. A relative `PATH` entry is ignored.
- `POST /api/clients/{client}/install` accepts the slugs `claude-code`, `codex`, `opencode-2` and `antigravity`, starts an install in the background, and returns `202` with a `ClientInstall`. An unknown slug returns 404. A second install for the same client while one runs returns 409. A service with no data directory returns 422.
- The core uses the vendor's install script and keeps the copy in a private folder under `<data dir>/clients/<client>`. It runs the installed copy's `--version` before it accepts the copy. A successful install replaces an older app copy. A failure removes the staged copy, keeps the older copy and sets `error`.
- `GET /api/clients/install/{id}` returns the current view. `state` is `running`, `done` or `failed`; `stage` and `step` follow the install. `path` and `version` are set when the install is done. An unknown id returns 404.
- The install runs only when a caller sends the request. The health check never installs a client.

## List endpoints

- `GET /api/projects?q=`, `GET /api/providers?q=` and `GET /api/configs?q=` return a plain array for the pickers.
- `q` is optional and case-insensitive. A missing or empty value returns every item.
- Projects match on name or folder. Providers match on name, vendor or plan. Configs match on name, client, model, provider name or vendor.
- Results are sorted by name, then id. Provider responses never include a key. The Host and Origin rules apply.

## GitHub

The GitHub connection uses the OAuth device flow. The service fetches the one-time code, polls GitHub in the background, and stores the token in the system keychain. The token is never in a response, a log or `state.json`. The client id comes from `ELESDEE_GITHUB_CLIENT_ID`; without one the sign-in returns 422. The API and login addresses come from `ELESDEE_GITHUB_API_BASE` and `ELESDEE_GITHUB_LOGIN_BASE`, and the default clone folder from `ELESDEE_GITHUB_DEFAULT_PARENT`; the environment overrides exist for the test fake server.

```ts
type GitHubDeviceState = "waiting" | "success" | "failed" | "cancelled";
type GitHubDeviceReason =
  | "notConfigured" | "network" | "denied" | "expired" | "keychain" | "error";

interface GitHubDeviceView {
  id: string;
  state: GitHubDeviceState;
  userCode: string | null;
  verificationUri: string | null;
  interval: number;
  expiresIn: number;
  login: string | null;
  reason: GitHubDeviceReason | null;
}

interface GitHubStatus {
  connected: boolean;
  login: string | null;
  name: string | null;
  avatarUrl: string | null;
  defaultCloneFolder: string;
}

interface GitHubRepo {
  id: number;
  fullName: string;
  name: string;
  owner: string;
  private: boolean;
  cloneUrl: string;
  htmlUrl: string;
  description: string | null;
  updatedAt: string | null;
  added: boolean;
}
```

- `GET /api/github/status` returns `GitHubStatus`. A stored token that GitHub rejects with 401 is deleted and the status reports disconnected. The default clone folder is `~/Projects` unless `ELESDEE_GITHUB_DEFAULT_PARENT` names another folder.
- `POST /api/github/device` starts the flow. It returns a waiting view with the code the user enters on GitHub. The background poll honors the interval, changes to `slow_down` for a longer wait, and ends as `success` with the account login, `failed` with a reason, or `cancelled`. `authorization_pending` keeps the flow waiting; `access_denied` gives `denied`; `expired_token` gives `expired`; a keychain write failure gives `keychain`. A network failure is retried, then reported as `network`. The flow expires after the time GitHub gives.
- `GET /api/github/device/{id}` returns the current view. An unknown id returns 404.
- `DELETE /api/github/device/{id}` cancels the flow and returns 204. An unknown id returns 404.
- `POST /api/github/sign-out` cancels open flows, deletes the keychain entry and clears the caches. It returns 204.
- `GET /api/github/repos?q=` returns `GitHubRepo[]` for the connected account. It needs a stored token; otherwise it returns 401. An empty `q` returns up to 50 recently updated repositories. A query filters by owner, repository name and description, and returns up to 50 matches. The service reads up to 300 repositories across three pages from `GET /user/repos` with owner, collaborator and organization affiliations, so private repositories appear when the token allows them. The list is cached for 60 seconds. `added` is true when a project folder's `origin` remote matches the repository; https, ssh and scp forms compare equal.
- `POST /api/github/clone` takes `{ fullName, name?, parentFolder? }`. It finds the repository in the connected account's list, clones its `cloneUrl` into `<parentFolder>/<repository name>`, and adds that folder as a project. `parentFolder` defaults to the default clone folder and is created when missing. The target folder must not exist; any existing file or folder returns 409 and is left unchanged. A failed or timed-out clone deletes the partial folder and adds no project; a failed state save removes the project and the folder. The clone runs for at most two minutes.
- The clone passes the token to Git through a private `GIT_ASKPASS` helper with mode 0700 and the token in the child environment. The remote URL in the repository is the ordinary clone address without the token. The helper is deleted after the clone. Clones run one at a time.
- A repository name that cannot be a folder name or an empty or malformed `fullName` returns 422. The clone address comes from the configured GitHub API, never from the caller. The real client accepts only https addresses; an address with another scheme fails the clone. The test configuration also accepts http and file addresses so a fake server can clone a local repository.

## Providers, AI configs and Antigravity

- `GET /api/provider-options` returns the vendor catalog: `{ id, label, auth, supportedClients, efforts, defaultModel, custom, signInClient }`. `efforts` maps a client name to its allowed levels. `signInClient` names the client that owns the vendor's subscription login, or `null` when the vendor has none.
- `POST /api/providers` adds a provider. The body is `{ name?, vendor, auth, key?, server?, apiStyle? }`. The name defaults to the vendor label. A key-mode provider needs a key; a local provider needs none. The key is stored in the system keychain through the secret store. It is not written to `state.json`, returned by the API or written to logs. A subscription provider created by this route has `connected: false`; it becomes usable through the sign-in route below.
- `GET /api/providers` returns `ProviderView[]`, which adds `supportedClients` to the stored record. Seed providers have `sample: true`. Providers created through the route have `sample: false`. `connected` says whether a subscription provider has a checked client sign-in; key and local providers are `true` when they hold their credential.
- `GET /api/providers/{id}/models?client=` reads the provider's model list in real time. OpenAI-style providers use `GET /models`; Anthropic-style providers use the documented model-list route or the known list; a local server uses its own `/models`. The result is `{ models: [{ id, label }], source, fallback }`. A successful read is cached briefly, concurrent reads for the same provider and client share one request, and a failed read returns the known list with `fallback: true`. The key is used only in the request and never appears in the response or a log. A typed custom model name is always allowed.
### Subscription sign-in

- `POST /api/sign-in` starts the installed client's own login. The body is `{ vendor, name?, providerId?, mode? }`. The vendor must be `anthropic` (Claude Code) or `openai` (Codex); other vendors return 422. `providerId` reconnects an existing subscription provider instead of adding one. `mode` is `browser` (default) or `code`; for Codex, `code` runs `codex login --device-auth`, while Anthropic uses one command and the user may paste the code it asks for.
- The core runs the client under a terminal so the client sees a real interactive login. It passes no token anywhere. It reads only the sign-in address and, for a device flow, the one-time code. It does not keep the raw output, read a credential file or log the output.
- The result is `SignInView`: `{ id, vendor, client, mode, state, url, code, reason, provider }`. `state` is `starting`, `waiting`, `verifying`, `success`, `failed` or `cancelled`. `reason` is `clientNotFound`, `clientError`, `notSignedIn`, `notSubscription`, `timeout`, `cancelled`, `verificationFailed` or `internal`.
- `GET /api/sign-in/{id}` returns the current view. The UI polls it while the attempt is active.
- `POST /api/sign-in/{id}/code` sends a one-time code to the client's input. The body is `{ code }`; the code is not stored.
- `DELETE /api/sign-in/{id}` abandons the attempt and stops the client process. Returns 204.
- When the login process exits with success, the core runs the client's status command: `claude auth status` for Claude Code and `codex login status` for Codex. Anthropic must report `loggedIn: true` and an `authMethod` of `claude.ai` or `oauth_token`; an `api_key` login returns `notSubscription`. Codex succeeds when the command exits 0.
- Only a successful check creates or updates the provider. The provider has `auth: "subscription"`, `connected: true`, `sample: false` and no key. `success` carries the provider as a `ProviderView`. A failed or abandoned attempt creates nothing. Reconnecting clears the saved usage rows, because another account may be behind the new sign-in.
- An attempt times out after 10 minutes. A vendor may have one active attempt at a time; a second start returns 409. One attempt runs per vendor and provider pair.

- A Google provider uses an API key. Antigravity runs `agy`. The core finds it on `PATH` and in common install paths. The default model for a new Antigravity config is `gemini-3.8-flash-medium`; model names use Antigravity's own slugs.
- For Antigravity, the core starts the client with a per-session home under the app's data folder, a private settings file that selects the Gemini provider, and the API key in the child environment. It removes the home when the session closes. Reopen creates it again and reads the key again.
- Antigravity sessions get no Claude hook settings and no permission card. Their `shortcuts` list is empty; the UI shows the base keys.
- `POST /api/configs` adds an AI config. The body is `{ name?, providerId, client, model?, effort?, args?, env?, config? }`. The provider must exist and support the client.
- `GET /api/configs` returns `AiConfig[]`.

## Usage sources

- Claude Code reports plan limits through a status line command that the core writes into the session's private settings file. The command posts the JSON that Claude Code sends on standard input to `/internal/statusline/claude/{id}` with the session's hook token. The core reads `rate_limits.five_hour` and `rate_limits.seven_day`, each with `used_percentage` and `resets_at`, for the provider usage. It also reads `context_window.remaining_percentage` for the session's context-left value and `session_id` for the agent's conversation ID.
- Codex reports plan limits over the session's app-server websocket. The core sends `initialize`, `initialized` and `account/rateLimits/read`, then applies `account/rateLimits/updated` notifications. Windows are told apart by length: 300 minutes is the 5-hour window and 10,080 minutes is the weekly window.
- DeepSeek reports a prepaid balance from `GET https://api.deepseek.com/user/balance` with the provider key from the keychain. The key is sent only in the request's `Authorization` header. It is never logged, returned or written to `state.json`.
- Other providers have no official source. The provider keeps `foot: "No official usage source."`, has no usage status and is skipped by auto-switch.
- A reading stores the rows, the window kind, `usageSource`, `usageReadAt`, `usageCheckedAt` and `usageStatus`. `usageStatus` is `fresh` after a successful read, `stale` when the last read failed but earlier values remain, and `unknown` when no value was ever read. A failed read sends a `provider.updated` event and leaves every session untouched.
- A fresh reading runs auto-switch immediately. A failed read never switches or pauses a session.
- DeepSeek balances are read in the background at most once a minute per provider and only for non-sample providers with a key. `POST /api/providers/{id}/usage/refresh` reads at once, but two concurrent reads for the same provider share one request, and a forced refresh within two seconds of the last attempt reuses the stored values.
- The manual route `POST /api/providers/{id}/usage` remains for a caller that supplies rows itself. It sets `usageSource: "manual"` and a fresh status.

## Edit and remove providers and configs

- `PATCH /api/providers/{id}` changes a provider. The body is `{ name?, vendor?, auth?, key?, server?, apiStyle? }`. An absent field keeps its value; `server` and `apiStyle` may be `null` to clear them. A supplied key replaces the saved key in the system keychain. A key-mode provider needs a stored key or a new one; otherwise the route returns 400. The route refuses with 409 when a config uses a client the edited provider no longer supports.
- The provider id does not change. An edited sample provider becomes normal with `sample: false`, and the mark stays gone after a restart. Usage rows stay when only the name, auth method or key change. They are cleared when the vendor, server address or API style changes, because the rows belong to another endpoint.
- `DELETE /api/providers/{id}` removes a provider. It deletes the keychain entry, the provider and its saved usage. It refuses with 409 while an AI config uses the provider, and the message says so. An unknown provider returns 404.
- `PATCH /api/configs/{id}` changes a config. The body is `{ name?, providerId?, client?, model?, effort?, args?, env?, config? }`. An absent field keeps its value; `model` and `effort` may be `null` to clear them. The provider and client pair must be supported.
- `DELETE /api/configs/{id}` removes a config. It refuses with 409 while an active session runs it, and the message names the session. It clears project defaults that use the config and removes it from every session's allowed list. An unknown config returns 404.
- Both edit routes save the state file. A provider edit sends a `provider.updated` event.

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
  connected: boolean;   // true while the phone holds a live link connection
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
- `GET /api/devices` adds `connected` from the live link state. It is not saved: a restart reports every device as disconnected until the phone reconnects.
- `DELETE /api/devices/{id}` returns 404 for an unknown id. It saves state and closes the device's live connections with link close code 3.
- The pairing handshake itself uses the pair ALPN, not these routes. It accepts one request and answers with a confirmation code, then a final `paired`, `denied` or `expired` message.

## Settings and diagnostics

```ts
interface AppSettings {
  startAtLogin: boolean;   // default false
  keepAwake: boolean;      // default false
}

interface SettingsPatch {
  startAtLogin?: boolean;
  keepAwake?: boolean;
}
```

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| PATCH | `/api/settings` | `SettingsPatch` | `AppSettings` |
| POST | `/api/diagnostics/export` | `{ folder }` | `{ path }` |

- `PATCH /api/settings` applies only the fields in the body and saves `state.json`. The desktop shell applies `startAtLogin` through the autostart plugin and `keepAwake` through the keep-awake plugin; the core stores the preference. `bootstrap` includes the current `settings`.
- `POST /api/diagnostics/export` writes one JSON bundle into the folder: `elesdee-diagnostics-<timestamp>.json`. The folder must be absolute and must exist; a relative path returns 400 and a missing folder returns 422. The bundle holds the app version, operating system, architecture, CPU count, record counts, the settings and at most the last 500 log lines. It holds no provider key, pairing secret, pairing ticket, agent credential or session output.
- The core writes a JSON log to `<data dir>/logs/core.log` with mode 0600. Each line is redacted before it is written. The file rotates to `core.log.1` at 4 MiB, and the bundle reads only the current file.
- Redaction replaces values whose key names a key, token, secret, password, credential, authorization, ticket or private key. It replaces bearer values, known credential prefixes, long hex, base32 and base64 runs, and values marked by `api_key=`, `token:`, `secret=` and similar. It removes session output, input, content, preview, stdout and stderr fields. The bundle applies the same rules again when it reads the log.

## Link app protocol

The routes above are local. A paired phone uses the `elesdee/app/1` transport. It opens one control stream and sends `hello` within five seconds. The computer answers `welcome` on the same stream. A connection that sends no compatible hello is closed.

```ts
interface AppHello {
  v: 1;
  bridgeVersion: number;   // the phone shell bridge
  protocolVersion: 1;
  platform: string;        // "android", "ios" or "desktop"
  appVersion: string;
  lastUiBuildId?: string;
}

interface Welcome {
  v: 1;
  computerId: string;      // z-base-32 endpoint id
  computerName: string;
  appVersion: string;
  protocolVersion: 1;
  minBridgeVersion: number;
  ui: unknown | null;      // the UI package manifest, or null
}
```

A phone checks the message version, the protocol version, the computer id against the stored address, and `minBridgeVersion`. A phone below the minimum shows an update message.

Every later stream carries one request with the shared four-byte JSON length frame. A unary method becomes an in-process request to the local router. The link sends `Host: 127.0.0.1`, no `Origin`, and `Content-Type: application/json` on a POST. The local request rules still apply.

| Method | Local route |
| --- | --- |
| `bootstrap` | `GET /api/bootstrap` |
| `health` | `GET /api/health` |
| `projects.list` | `GET /api/projects?q=` |
| `branches.list` | `GET /api/branches?folder=&q=` |
| `providers.list` | `GET /api/providers` |
| `accounts.list` | `GET /api/accounts?q=` |
| `sessions.create` | `POST /api/sessions`, with any `folder` removed |
| `sessions.close` | `POST /api/sessions/{id}/close` |
| `sessions.input` | `POST /api/sessions/{id}/input` |
| `sessions.permission` | `POST /api/sessions/{id}/permission` |
| `sessions.account` | `POST /api/sessions/{id}/account` |
| `sessions.config` | `POST /api/sessions/{id}/config` |
| `sessions.resume` | `POST /api/sessions/{id}/resume` |
| `questions.list` | `GET /api/questions` |
| `questions.answer` | `POST /api/questions/{id}/answer` |
| `sessionRequests.approve` | `POST /api/session-requests/{id}/approve` |
| `sessionRequests.deny` | `POST /api/session-requests/{id}/deny` |
| `files.list` | `GET /api/sessions/{id}/files?path=` |
| `files.read` | `GET /api/sessions/{id}/file?path=` |
| `diff.read` | `GET /api/sessions/{id}/changes` |
| `events.subscribe` | `GET /api/events`, streamed |
| `output.subscribe` | `GET /api/sessions/{id}/output?after=`, streamed |

- The result is the local route's JSON, or the local error message with the matching link code. A method outside the allowlist returns `forbidden` before a handler runs.
- `sessions.config` switches to one exact AI config and needs `{ sessionId, configId }`. It replaces the provider alias for the phone. `sessions.account` stays for an older client and maps a provider id to that provider's config for the session's client.
- `sessions.upload`, `files.image`, `ui.manifest` and `ui.download` are allowlisted but return `unsupported` until their steps. `demoServers.list` returns the active registered entries.
- `events.subscribe` has no parameters. It forwards `session.updated` from the event stream and the question, request, notification, shared-file and provider events from the auxiliary stream. A phone that falls behind loses the stream and reconnects; the bootstrap snapshot shows the current state.
- `output.subscribe` takes `{ sessionId, after }`. It sends a stream start, replays the retained output with a higher sequence than `after`, and keeps the stream open. A replayed event may carry `gap`. The stream ends when the session closes. A slow reader gets a fresh snapshot from its last sequence, which fills the gap.
- The link holds the same output stream permit as the local API and applies its own stream cap.
- A revoked device has its live connections closed with code 3.

The phone shell reconnects after a drop. It waits 0.5 s, 1 s, 2 s, 4 s, 8 s, 15 s and then 30 s, with ±20% jitter. A connection that stayed up for 30 seconds resets the sequence. The shell reopens the event stream and resumes each output stream from the last sequence seen. A revoked or unknown device stops the retries.

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

- Claude Code, Codex and OpenCode 2 sessions get the elesdee MCP server and the skill pack: `elesdee`, `demo-servers`, `handoff`, `ask-and-notify`, `coordinate-sessions`, `frontend-design` and `browser-checks`. Antigravity gets neither.
- The server exposes `ask_me`, `wait_for_answer`, `withdraw_question`, `notify_me`, `report_done`, `write_handoff_note`, `list_sessions`, `read_session`, `send_message`, `check_usage`, `start_session`, `wait_for_session`, `share_file`, `register_demo_server`, `browser_open`, `browser_read`, `browser_click`, `browser_type`, `browser_wait` and `browser_screenshot`.
- `wait_for_session` waits on another session's status and work state. `until` is `attention` (default: input, permission, question, pause, done or closed), `waiting`, `done`, `closed` or `change`. `timeoutSeconds` is 1–25. The call returns the session state and `timedOut`; it never blocks past the route timeout. It stops with an error when the calling session closes.
- `share_file` takes a path relative to the session folder and an optional caption. It copies the file into the shared store and returns the `SharedFile` record. A source that resolves outside the folder, a folder, a hard link, a secret file name, private-key content or a stored provider key in the content is refused.
- The core writes each session's skill kit under `<data dir>/skills/<session id>`. Directories use mode 0700 and files use mode 0600. The kit is removed on close, agent exit, delete and shutdown, and a startup sweep removes stale kits.
- The kit holds `.claude-plugin/plugin.json` and `skills/<name>/SKILL.md`. Claude Code gets it as `--plugin-dir <kit>`.
- OpenCode 2 gets `<data dir>/mcp/opencode-<session id>.json`. Its `skills` array lists `<kit>/skills`, and `OPENCODE_CONFIG` points at the file. The file is removed with the session.
- Codex gets `CODEX_HOME=<kit>/codex` in its launch environment. Its skills are written under `<kit>/codex/skills`. When the user's Codex home has `auth.json` or `config.toml`, the launch links the file into the private home so the client sign-in and configuration still apply. The private home is removed with the kit.
- The launch does not write into the repository or change the user's global agent files.
- The `write_handoff_note` tool saves the handoff note on the session. The note is at most 8000 characters. A closed session refuses the tool. Saving the note emits `session.updated` and writes the state file.
- A switch to a different client sends the saved note and the end of the output log to the new agent as its first input. A switch inside the same client resumes the conversation and sends no context. The log tail is plain text, capped at 4000 characters; the whole context is capped at 6000 characters.
- A pause keeps the note. A later cross-client switch from the paused session sends it with the log tail.
- Reopening a closed agent session without a saved conversation ID sends the note and the log tail after the client starts.

## Session browser

- Each session has its own Chromium process and its own profile folder at `<data dir>/browsers/<session id>`. The route set below uses only that session's context. A tab id includes the session, so a tab id from another session is unknown.
- The process starts on the first action. `GET` state on a session that never started returns `started: false` and no tabs.
- A closed session returns 409 for every browser action. Its state route still answers, with no tabs and `started: false`.
- Only `http` and `https` addresses are accepted. A missing or malformed address, a relative address or another scheme returns 400. A session context holds at most 12 tabs; a further tab returns 409.
- Close and pause stop the process and clear the tabs. The profile stays on disk. The core stops every browser at shutdown. The profile is not encrypted yet.
- The state shape is `{ sessionId, started, available, tabs, activeTabId, screenshotSeq, error }`. Each tab is `{ id, url, title, loading, canGoBack, canGoForward }`. `screenshotSeq` changes when a new screenshot is captured; the UI uses it to refresh the image.
- The screenshot route returns the latest PNG for the given tab, or the active tab when `tab` is absent. It returns 404 when there is no page or no capture yet.
- The six agent tools use the calling session's context and take no session id. A closed session gets an error tool result. `browser_open` and `browser_read` start the browser when needed. `browser_screenshot` writes a PNG into the shared file store and returns the `SharedFile` record.
## Demo servers

```ts
type DemoServerState = "active" | "stopped";

interface DemoServer {
  id: string;
  sessionId: string;
  name: string;
  port: number;
  address: string;     // "http://localhost:3000"
  state: DemoServerState;
  createdAt: string;   // RFC 3339
}

interface DemoServerListEntry extends DemoServer {
  sessionName: string;
}
```

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| GET | `/api/demo-servers` | | `DemoServerListEntry[]` for active servers |
| POST | `/api/sessions/{id}/demo-servers/{serverId}/stop` | `{}` | `Session` |

- `register_demo_server` is an MCP tool, not a local route. It takes a name and a port and returns the local address. The tool refuses with `isError` when the session is closed, the name is empty, the port is outside 1024–65535, the port is the core service's port, another active session registered the port, or the session already holds 8 servers.
- The name is cleaned of control and invisible characters and capped at 64 characters. A repeated port in the same session updates its name and makes it active again. The record id and creation time stay.
- `GET /api/demo-servers` returns only active records. The array is empty when no server is registered. Records are ordered by session order and then registration order.
- The stop route returns the updated session with `state: "stopped"`, emits `session.updated`, and closes that port's forward tunnels. A second stop returns the same stopped record.
- An unknown session or server returns 404. A closed session refuses a registration through the tool, not through the stop route.
- The records live with the session and appear in `bootstrap`. They are saved in `state.json`. On restart every saved record becomes `stopped`, because the agent process and its servers no longer run.
- Close and pause set every server in the session to `stopped` and close its tunnels. Delete removes the session and its records.
- The link method `demoServers.list` maps to `GET /api/demo-servers`. The phone receives only active ports. Forwarding itself uses the separate `elesdee/forward/1` ALPN; it is not an RPC method.

## Switching AI configs and context

- `GET /api/sessions/{id}/switch-options` returns every AI config on the computer, the session's `allowedConfigIds` and `nextConfigId`: the allowed config the automatic switch would pick next. A terminal session returns 422; an unknown session returns 404.
- `POST /api/sessions/{id}/config` switches the session. The body is `{ configId }`. The session must be an active agent session, the config must exist, and it must be in the session's allowed list. A config outside the list returns 422. `POST /api/sessions/{id}/account` remains as an alias for the phone: it maps a provider id to that provider's config for the session's client.
- A switch inside the same client resumes the client's conversation. A switch to a different client starts the new client with the saved handoff note and the end of the output log. A switch clears `contextLeft` because the new process reports its own figure later.
- Automatic switch runs when a provider reaches 90% of a plan limit or falls below its prepaid minimum. It picks only allowed configs whose provider has room and never a sample. Subscription providers come first, then pay-per-use providers. Within a group, the provider with the most room wins; a tie keeps the session on its current client. A different client gets the handoff note and the end of the log. If no allowed config has room, the session pauses with a note, and it resumes when an allowed config has room again.
- A new agent session with no `allowedConfigIds` allows every non-sample config of its starting client, plus the starting config. The launch form sends its own list. The list holds existing config ids, has no duplicates, and must include the current config. A terminal session may not send a list. `PATCH /api/sessions/{id}` changes the list on an active agent session; removing the current config returns 422, an empty list returns 400, and an unknown config returns 422.
- `contextLeft` is the percentage of the client's context window that is still free. For Claude Code it comes from the status line JSON, field `context_window.remaining_percentage`, rounded and clamped to 0–100. Other clients report no figure yet, so the value stays `null`.
- The Claude settings file adds a `statusLine` command that sends the JSON to `POST /internal/statusline/claude/{id}?token=...`. The route uses the same per-session token as the hooks. A wrong token or a non-Claude session returns 403. It accepts `{ session_id?, context_window? }`, updates the session only when the rounded value changes, sends `session.updated`, and returns 204.
- A missing or null `remaining_percentage` sets `contextLeft` to `null`. A switch, resume or restart recovery clears the value.

## Persistence

- State lives in `ELESDEE_DATA_DIR`, default `~/.elesdee`, in `state.json`. The directory is created with mode 0700 and files with mode 0600. Writes use a temporary file, fsync and rename.
- Projects and their default AI config and source branch, providers, AI configs, sessions, each session's desired columns and rows, its output log, an agent's repository root, an agent's own conversation ID, the session's handoff note and last context-left figure, the session's allowed config ids, the paired device list and the General settings are saved. Provider keys and pairing secrets are not saved. No runtime handles are saved.
- The log file is `<data dir>/logs/core.log`, with one backup at `core.log.1`. It is not part of `state.json` and holds redacted JSON lines only.
- The desktop link identity is a separate 32-byte key in `<data dir>/link.key`, with mode 0600, written once and reused. It is never in `state.json` and never logged. A missing key creates a new identity; every paired phone must pair again.
- The paired device list is written on pair, removal and graceful shutdown. `lastSeen` updates in memory at most once a minute.
- Shared files live in `<data dir>/shared/<session id>/`, directories 0700 and files 0600. Each copy has a metadata record beside it with the original relative path, name, caption, size and content type. The stored copy stays after the session closes. Provider keys are compared with the content and are never copied.
- State is saved on project create, provider create, provider edit, provider removal, a successful subscription sign-in, config create, config edit, config removal, session create, rename, close, reopen, delete, resize, graceful shutdown, and when an agent reports its conversation ID. It is not written for each output chunk.
- A session that was active on save is restarted when the state loads. It resumes the agent's conversation by its saved `agentSessionId`; without an ID it uses the client's continue-last behavior and the preview says so. A terminal session restarts its shell in the same folder. The log and sequence numbers are kept, so new output continues from the next sequence.
- If the session folder, AI config or client is missing, that session is restored as active and paused with a clear reason. The other sessions still restart. A pending permission request is not written to the file and is not restored.
- `agentSessionId` comes from the client. Claude Code gets a generated `--session-id` at launch and also reports it in hook input. Codex reports the app-server thread ID. OpenCode reports the session ID on its local server. Antigravity has no ID and uses its continue-last behavior.
- A new state starts with no projects. Seed providers and configs are used only when no state file exists or when the state file is corrupt. An older state file with accounts and presets is migrated: each account becomes a provider and a config, each preset becomes a config, and sessions map their account and preset onto the best config. A corrupt file is moved to `state.bad.json` and the problem is logged.

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
  configId: string | null;       // the AI config the session runs
  providerId: string | null;     // the provider of that config
  allowedConfigIds: string[];    // the configs the session may use, including the current one
  agentSessionId: string | null; // the client's own conversation ID
  handoffNote?: string | null;   // kept for the next agent that works on the session
  contextLeft: number | null;    // percent of the context window free, or null when unknown
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  envOverrides: EnvOverride[];   // the launch environment
  model: string | null;          // the config's model at launch
  effort: string | null;         // the config's effort at launch
  args: string[];                // the config's extra arguments
  config: Record<string, string>; // the config's overrides at launch
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

interface Provider {
  id: string;                    // unique slug of the name
  name: string;
  vendor: string;                // anthropic, openai, google, deepseek, custom, ...
  auth: "subscription" | "key" | "local";
  server: string | null;         // base URL when the vendor or custom entry needs one
  apiStyle: "openai" | "anthropic" | null;
  sample: boolean;               // seed provider, not added by the user
  connected: boolean;            // a subscription provider has a checked client sign-in
  plan: string;
  rows: UsageRow[];
  foot: string;                  // "No official usage source." when none exists
  usageReadAt: string | null;    // RFC 3339, last successful read
  usageCheckedAt: string | null; // RFC 3339, last attempt
  usageStatus: UsageStatus | null;
  usageSource: "statusline" | "app-server" | "deepseek" | "manual" | null;
}

interface ProviderView extends Provider {
  supportedClients: Client[];    // computed from the vendor and API style
}

interface AiConfig {
  id: string;                    // unique slug of the name
  name: string;                  // defaults to "<Provider> (<Client>)"
  providerId: string;
  client: Client;
  model: string | null;
  effort: string | null;
  args: string[];                // extra command-line arguments, one per entry
  env: EnvOverride[];
  config: Record<string, string>; // keys may use dots for nested settings
  createdAt: string;
  updatedAt: string;
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
  defaultConfigId: string | null;
  defaultSourceBranch: string | null;
}

interface SwitchOptions {
  configs: AiConfig[];           // every AI config on this computer
  allowedConfigIds: string[];    // the configs the session may use
  nextConfigId: string | null;   // the allowed config auto-switch would pick next
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

interface AgentCheck {
  name: Client;
  installed: boolean;
  version: string | null;
  path: string | null;    // the executable the check runs
}

interface ClientInstall {
  id: string;
  client: Client;
  state: "running" | "done" | "failed";
  stage:
    | "preparing"
    | "downloading"
    | "installing"
    | "verifying"
    | "finishing";
  step: number;           // 1–5
  total: number;          // 5
  version: string | null;
  path: string | null;
  error: string | null;
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
| POST | `/api/clients/{client}/install` | `{}` | `202` `ClientInstall` |
| GET | `/api/clients/install/{id}` | | `ClientInstall` |
| GET | `/api/bootstrap` | | `{ projects, providers, configs, sessions, questions, sessionRequests, settings }` |
| GET | `/api/projects?q={text}` | | `Project[]` |
| POST | `/api/projects` | `{ name, folder }` | `Project` |
| PATCH | `/api/projects/{id}` | `{ defaultConfigId?, defaultSourceBranch? }` | `Project` |
| GET | `/api/git-status?folder={folder}` | | `{ git, dirty }` |
| GET | `/api/providers?q={text}` | | `ProviderView[]` |
| GET | `/api/provider-options` | | `ProviderOption[]` |
| POST | `/api/providers` | `{ name?, vendor, auth, key?, server?, apiStyle? }` | `ProviderView` |
| PATCH | `/api/providers/{id}` | `{ name?, vendor?, auth?, key?, server?, apiStyle? }` | `ProviderView` |
| DELETE | `/api/providers/{id}` | | `204` |
| GET | `/api/providers/{id}/models?client={client}` | | `{ models, source, fallback }` |
| GET | `/api/providers/{id}/usage` | | `ProviderView` |
| POST | `/api/providers/{id}/usage` | `{ rows: UsageRow[] }` | `ProviderView` |
| POST | `/api/providers/{id}/usage/refresh` | `{}` | `ProviderView`; 502 when the read fails |
| POST | `/api/sign-in` | `{ vendor, name?, providerId?, mode? }` | `SignInView` |
| GET | `/api/sign-in/{id}` | | `SignInView` |
| POST | `/api/sign-in/{id}/code` | `{ code }` | `SignInView` |
| DELETE | `/api/sign-in/{id}` | | `204` |
| POST | `/internal/statusline/claude/{id}?token={token}` | the Claude Code status line JSON | `204` |
| POST | `/api/pairing` | `{}` | `{ expiresAt, relayReady }` |
| GET | `/api/pairing/qr` | | `image/png` |
| GET | `/api/pairing` | | `PairingStatus` |
| POST | `/api/pairing/approve` | `{}` | `PairedDevice` |
| POST | `/api/pairing/deny` | `{}` | `204` |
| DELETE | `/api/pairing` | | `204` |
| GET | `/api/devices` | | `PairedDevice[]` |
| DELETE | `/api/devices/{id}` | | `204` |
| PATCH | `/api/settings` | `SettingsPatch` | `AppSettings` |
| POST | `/api/diagnostics/export` | `{ folder }` | `{ path }` |
| GET | `/api/configs?q={text}` | | `AiConfig[]` |
| POST | `/api/configs` | `{ name?, providerId, client, model?, effort?, args?, env?, config? }` | `AiConfig` |
| PATCH | `/api/configs/{id}` | `{ name?, providerId?, client?, model?, effort?, args?, env?, config? }` | `AiConfig` |
| DELETE | `/api/configs/{id}` | | `204` |
| POST | `/api/sessions` | `{ projectId, kind, configId?, allowedConfigIds?, name?, folder?, sourceBranch?, copyUncommittedChanges?, env?, cols?, rows? }` | `Session` |
| POST | `/api/sessions/{id}/close` | `{}` | `Session` |
| PATCH | `/api/sessions/{id}` | `{ name?, allowedConfigIds? }` | `Session` |
| POST | `/api/sessions/{id}/reopen` | `{}` | `Session` |
| POST | `/api/sessions/{id}/config` | `{ configId }` | `Session` |
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
| GET | `/api/sessions/{id}/browser` | | `BrowserState` |
| POST | `/api/sessions/{id}/browser/start` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/stop` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/open` | `{ url }` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/back` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/forward` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/reload` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/tabs` | `{ url? }` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/tabs/{tab}/activate` | `{}` | `BrowserState` |
| POST | `/api/sessions/{id}/browser/tabs/{tab}/close` | `{}` | `BrowserState` |
| GET | `/api/sessions/{id}/browser/screenshot?tab={tab}` | | `image/png`; 404 when no capture is available |
| GET | `/api/sessions/{id}/output?after={seq}` | | SSE: `data: {"seq":n,"data":"..."}` and an optional `gap` |
| GET | `/api/events` | | SSE: `data: {"type":"session.updated","session":Session}` |

Errors use `{ "error": "message" }`. Pairing returns 503 when the link endpoint is not running, and 409 when no phone waits for approval or the device limit is reached. Removing an unknown device returns 404. Unknown sessions return 404. A session that is closed, or whose process has exited, returns 409 for input. An invalid session name or terminal size returns 400. An agent session in a non-Git folder returns 422. Reopening an active session, deleting an active session, or reopening or deleting while a close is running returns 409; reopening with a missing folder, repository, branch or client returns 422. A permission answer returns 404 for an unknown session and 409 when the session is closed, there is no pending request, the request id does not match, or the request was already answered. An empty project name returns 400, a bad project folder returns 422, and a duplicate project folder returns 409. An unknown project returns 404. A project change that cannot be written to disk returns 500 and is rolled back in memory. A default AI config that does not exist, a default branch that does not exist, or a project folder that is not a Git repository returns 422. An unsafe branch name returns 400. An empty provider name, empty key or overlong model returns 400; an unsupported vendor, client pair or auth mode returns 422; a keychain failure returns 500. A sign-in for an unsupported vendor returns 422; an invalid provider name or code returns 400; a second active sign-in for the same vendor and provider returns 409; an unknown sign-in returns 404. An unknown provider or a model or effort that does not fit the client returns 422; an invalid config name, environment variable or config key returns 400; a secret-like environment variable or config key returns 422; an unknown config returns 404. Removing a provider that a config uses returns 409; removing a config that an active session runs returns 409. An allowed config list that is empty returns 400; an unknown config, a list without the current config, or a switch to a config outside the list returns 422; changing the list on a closed session returns 409. A blocked Host or Origin returns 403, a bad POST or PATCH content type returns 415, a full session list or stream list returns 429, a request that exceeds the 30-second timeout returns 408, and a wrong non-OPTIONS method returns 405. A file path outside the session folder returns 400, a binary file returns 422, and content above 1 MiB returns 413. A closed session returns 409 for a browser action, an unknown tab returns 409, an invalid or non-http address returns 400, an unavailable browser returns 503, and a browser action that fails returns 502. A demo-server stop for an unknown session or server returns 404; a registration on a closed session is refused by the tool.

## Rules

- A terminal session runs a real shell in its folder through a PTY. Closing the session stops the process group and keeps the folder.
- Close commits leftover work on the session branch and pushes it. When the session has a handoff note, the commit message body carries the note. A clean tree keeps its last commit; if that commit does not carry the note, Close adds an empty commit that does. The note is never written as a file.
- The file and change routes use the session folder: the worktree for an agent, the chosen folder for a terminal. A missing folder returns 404.
- A file path is relative to the session folder and is canonicalized. An absolute path, a `..` component and a symlink that resolves outside the folder return 400. A folder where a file is expected returns 422. A path with a `.git` component is refused, as is a hard-linked file and a named pipe or other special file. The listing skips `.git` folders, symlinks and names that are not UTF-8; a skipped name marks the listing truncated.
- A shared file is copied from the session folder, never linked to the source. The store holds at most 10 MiB per file. The list is newest first. The read route returns 404 for an unknown id, 413 above the cap and 500 when the store is unavailable. Images are served inline; other files are served as attachments with a sanitized name. The response is not cached.
- A browser profile belongs to one session and is reused across starts. Two sessions never share cookies, storage or logins. The process is stopped on close, pause and shutdown, and the profile folder is not removed on delete yet.
- One folder listing returns at most 2000 entries and sets `truncated` when more exist. A file read or write is UTF-8 text of at most 1 MiB. A larger file or request body returns 413. Binary content, meaning a NUL byte in the first 8 KiB, returns 422.
- `POST /api/sessions/{id}/file` creates the file or replaces its content. The parent folder must exist. The write is not limited to existing files. A write goes to a temporary file in the same folder and is renamed into place, so a failure cannot leave a half-written file. An existing file keeps its permission bits.
- Changes for an agent session compare the worktree, including uncommitted work, against the merge base with the repository's current branch. Changes for a terminal session compare the working tree against `HEAD`. Untracked files are included.
- The change route caps the list at 500 files and the combined diffs at 512 KiB. It sets `truncated` when either cap is reached. A folder outside Git returns `git: false` with no files and no error.
- An agent session runs the chosen client when one is installed. The client command is chosen by the server, never sent by the UI.
- The server tells the UI the client's key shortcuts. The UI keeps no list of client keys. An Antigravity session gets an empty list, so the UI shows the base keys.
- Every change to a session sends `session.updated` on `/api/events`.
- Every usage reading, successful or failed, sends `provider.updated` on `/api/questions/events` with the full provider view. The desktop UI replaces that provider in its list; no other provider field changes.
- Output events carry a sequence number. `after=n` replays the output with a higher sequence number.
- `after` must be at most the newest sequence. A greater value returns 400.
- The retained log is bounded. When old chunks have been dropped, the first replayed event carries `gap` with the oldest retained and newest known sequence, so the UI can show that text was lost.
- Both SSE endpoints send an initial comment and a comment every 15 seconds while idle. An output stream ends when its session closes.
- The output log is bounded per session. Closed session logs share a bounded memory budget; the oldest closed logs are dropped first.
