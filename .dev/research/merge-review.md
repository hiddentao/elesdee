# Merge review: presets, recovery, skill pack, phone shell

Range: `b4c1973..8ec30a1` on `hiddentao/round-2-mockups`. This covers the merges for editable launch presets and config overrides (`3f0345a`), crash recovery (`35665e0`), the session skill pack (`7a4de16`) and the phone shell client (`8ec30a1`).

Method: code reading against `.dev/SPEC.md`, `.dev/api.md` and `.dev/phone-shell.md`, the core and link test suites, and one standalone permission check. No code was edited.

Tests run: `cargo test` in `core/` passes (127 tests). `cargo test` in `link/` passes (unit tests, 5 client tests, 1 loopback test).

## Findings

### 1. Medium: crash recovery drops a Codex session's model and config overrides

- Where: `core/src/recovery.rs:73-152` (the agent arm of `restart`). Compare `core/src/api.rs:1473-1480` (create), `core/src/api.rs:1790-1795` (reopen) and `core/src/api.rs:2022` (account switch), which all call `apply_launch_arguments`.
- Scenario: create a Codex session from a preset with `model: "gpt-5.2"` and `config: {"model_reasoning_effort": "high"}`. Quit or crash, then start the core again. Recovery rebuilds the command with `resume <thread>` and the MCP flags, but never adds `-c model=...` or `-c model_reasoning_effort=...`. The session resumes its conversation on the default model and default settings. `.dev/SPEC.md:298` says a restart relaunches each session "with the same settings".
- Claude, OpenCode 2 and Antigravity are unaffected because their overrides go through the settings or config files, which recovery does rewrite.
- Fix: call `apply_launch_arguments(client, session.model.as_deref(), &session.config, &mut command)` in the agent arm before `configure_session_tools`. Make the helper `pub(crate)` or move it to a shared module.

### 2. Medium: the phone shell holds its command mutex across network waits

- Where: `phone-shell/src-tauri/src/commands.rs:224-250` (`phone_pair_start`), `307-324` (`phone_connect`) and `338-368` (`phone_request`). In each case `state.inner.lock().await` is held while awaiting the link client.
- Scenario: the paired computer becomes unreachable or stops reading. The user taps a computer to open its sessions. `phone_connect` holds the mutex for up to the 30-second connect timeout, and then `phone_request` holds it for up to another 30 seconds. During that time every other command queues behind the lock: the computers screen cannot refresh, Forget and Disconnect do nothing, and pairing cannot start. If the write path blocks (see finding 3), the lock is held indefinitely and the shell needs a restart.
- The same pattern makes Cancel on the pairing screen unable to run while the initial connect and confirm reads are in flight.
- Fix: take the record or connection out of the map under the lock, drop the guard, then await the network call. `iroh::endpoint::Connection` is cloneable, so `AppConnection` can be made `Clone` and used outside the lock. Re-lock only to insert or remove map entries.

### 3. Low: the phone request timeout covers only the response read

- Where: `link/src/client.rs:426-451`. `open_bi`, `send.write_all` and `send.finish` run without a timeout; only `read_response` is wrapped in `UNARY_REQUEST_TIMEOUT`. The pairing dial has the same shape at `link/src/client.rs:591-651`: connect and the first read are timed, but `open_bi` and `write_json` are not.
- Scenario: a paired computer accepts the QUIC connection but does not open streams or does not read. Flow-control credit never arrives, `open_bi` or `write_all` waits forever, and `.dev/phone-shell.md` promises a 30-second unary request timeout. Combined with finding 2, the phone shell freezes.
- Fix: wrap the whole exchange (`open_bi`, write, finish, read) in one `timeout(UNARY_REQUEST_TIMEOUT, ...)`. Do the same in `pair_at`.

### 4. Low: restored active sessions are not counted toward the active-session cap

- Where: `core/src/state.rs:1428-1455` (`restore_session`) inserts entries without touching `store.active`; `core/src/state.rs:1396-1415` and `1505-1528` use that counter for the cap and for close/release.
- Scenario: 20 sessions were active at the last save. After a restart all 20 are recovered and active, but `store.active` is 0. The user can then start 64 more sessions, so 84 processes run. The documented cap is 64 (`.dev/api.md:26`). Closing a recovered session subtracts from 0 and leaves it at 0.
- Fix: in `restore_session`, after inserting, increment `store.active` when the entry is active. Do not use `insert_session`, because recovery must not drop sessions over the cap.

### 5. Low: a Codex session's skill kit root is not created with mode 0700

- Where: `core/src/mcp.rs:576-583`. `prepare_codex_home` calls `create_private_dir_all` on `<data>/skills/<session>/codex` only. `fs::create_dir_all` creates `<data>/skills` and `<data>/skills/<session>` with the process umask, and `create_private_dir_all` then changes only the `codex` directory to 0700. Claude and OpenCode sessions call `prepare_skill_kit`, which creates and restricts the session kit root.
- Verified with a standalone program using the same standard-library calls at umask 022: the result is `skills` 0755, `skills/session-1` 0755, `skills/session-1/codex` 0700. `.dev/api.md:128` says directories in the kit use mode 0700.
- Impact is limited because the data directory itself is 0700, but the kit's own directory does not meet the stated rule.
- Fix: in `prepare_codex_home`, call `create_private_dir_all` on `skill_kit_path(data_dir, session_id)` first, then on the `codex` child. Optionally create `<data>/skills` with mode 0700 in both paths.

### 6. Low: crash recovery does not check that an agent worktree is still valid

- Where: `core/src/recovery.rs:57-60` checks only `folder.is_dir()`. `.dev/SPEC.md:300` says a missing worktree leaves the session paused with the reason.
- Scenario: the user runs `git worktree prune` or removes the worktree's administrative entry while the folder remains. On the next start the folder still exists, so recovery spawns the agent in a directory that is no longer a worktree. Git commands inside it fail, and the preview says "Resumed." Reopen already handles this case through `ensure_worktree`, which replaces a stale or unregistered path.
- Fix: for agent sessions, check that the folder is a registered worktree on the stored branch before spawning (a blocking `git rev-parse --is-inside-work-tree` and `--abbrev-ref HEAD` is enough). If the check fails, pause with a clear reason. Do not delete or recreate the worktree during recovery unless the product decides to treat a missing worktree like reopen.

### 7. Low: the phone does not check that the pairing reply names the endpoint it dialed

- Where: `link/src/client.rs:228-286` (`PairingMachine::accept_confirm`) takes `computerId` from the reply verbatim and stores the ticket address as a separate field. `.dev/phone-shell.md:105` says the computer id is the endpoint id.
- Scenario: a stale or misbehaving desktop reply carries a different `computerId`. The phone stores a record whose id is not the endpoint it will dial. The id keys the computer list, the connection map and Forget, so the record can mismatch the actual computer and display the wrong identity. The connection is still protected by iroh's endpoint authentication; this is an integrity and display error, not an impersonation path.
- Fix: compare the reply id with `self.addr.id.to_z32()` inside `accept_confirm` and reject a mismatch as a protocol error. Add a unit test.

### 8. Low: copies of the pairing secret in the serialized hello are not zeroized

- Where: `link/src/client.rs:616-627` zeroizes `hello.secret` after `write_json`, but `write_json` (`link/src/client.rs:731-743`) creates a `serde_json::to_vec` buffer and a frame buffer that both contain the base32 secret. Those buffers are dropped without zeroization, and `write_all` may copy them again.
- Scenario: the pairing secret is designed to be cleared with `zeroize` (`.dev/phone-shell.md:26`). A memory dump or swap after pairing can still contain the serialized copy. The ticket object itself zeroizes on drop, so this is the remaining plaintext copy.
- Fix: serialize into a `Zeroizing<Vec<u8>>` (or explicitly zeroize `bytes` and the frame after the send) in `write_json`, and zeroize the request frame in `AppConnection::request` if it can carry secrets.

### 9. Low: the `elesdee` skill's tool table omits `write_handoff_note`

- Where: `core/assets/skills/elesdee/SKILL.md:16-31` lists the MCP tools and leaves out `write_handoff_note`. The tool exists (`core/src/mcp.rs:88-102`, `167`, `296`) and the handoff skill tells the agent to call it (`core/assets/skills/handoff/SKILL.md:34`).
- Scenario: an agent follows the main skill's table as the complete tool list, skips the handoff note, and then the app cannot put the note in the close commit or give it to a new client after a switch.
- Fix: add a `write_handoff_note` row to the table. Check the table against `tools()` in `mcp.rs` in a test so the two cannot drift again.

### 10. Low: docs say API keys are never part of a preset, but preset environment values are stored in plain text

- Where: `docs/settings.md:21`; storage at `core/src/state.rs:171` (`env_overrides`) and `core/src/state.rs:1465-1490` (`try_save` writes the session), with presets saved at `core/src/persist.rs:24`. `GET /api/presets` returns `env` values unchanged.
- Scenario: a user reads the docs, then types `ANTHROPIC_API_KEY=sk-...` into a preset's environment variables to avoid a sign-in. The value is written to `state.json` in plain text and returned by the API on every preset bootstrap. This contradicts the hard rule that keys live in the keychain and are never part of a preset.
- Impact: the app never copies an account key into these fields; only a user can do it. But the docs make an absolute promise that the code does not enforce.
- Fix: either reject or warn on environment names that look like secrets in presets and per-launch overrides, or amend the docs to say that user-entered environment values are stored as written and must not contain keys. The first option fits the existing hard rule better.

## Checked and correct

- Preset config never writes into a repository. Claude settings, the Claude MCP file, the skill kit, the OpenCode config, the Codex home and the Antigravity home are all under the data directory. The worktree receives no elesdee files.
- Account keys stay in the secret store. `providers::environment` puts them only into the child environment, and `secret_environment_values` covers every current key variable name (each contains `KEY` or `TOKEN`) for output redaction.
- Claude hooks are inserted after config overrides and cannot be replaced by a preset. OpenCode's `mcp` and `skills` entries and the model are inserted after the overrides.
- Preset creation, update and delete match `.dev/api.md:18-25`, including the `account:` derived presets, the account-match rule, the `model: null` clear via `optional_field`, and the validation limits.
- Crash recovery resumes the saved conversation: it uses `--resume <id>` for Claude, `resume <id>` for Codex and `--session <id>` for OpenCode, and falls back to the continue-last flags with the matching preview when no id exists. The agent conversation id is persisted and restored.
- The skill kit holds all five skills, its files are 0600, it is removed on close, agent exit, delete and shutdown, and a startup sweep removes stale kits. The Codex auth file is linked, not copied.
- The phone client checks the declared pairing frame and response header and body lengths before allocating, rejects an unsupported protocol version, parses every interface method through the shared allowlist in both the shell command and the core request decoder, and maps unknown methods to `forbidden`. The request body and header limits are enforced on encode as well as decode.
- The phone identity is read from and written to the platform keychain; the computer list holds no secret and is written with mode 0600.
