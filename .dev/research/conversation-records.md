# Can a separate program read a live Claude Code / Codex CLI conversation in structured form?

Research date: 18 November 2026. Goal: a desktop app launches the agents in an interactive terminal on the Mac, and a phone shows a simple chat-style view of each session.

## How the pages were fetched

- **Claude Code:** pages fetched from `https://code.claude.com/docs/en/<page>`. Each page also has a Markdown twin at `<page>.md`, listed in `https://code.claude.com/docs/llms.txt`. Quotes come from those Markdown twins.
- **Codex:** `https://developers.openai.com/codex/` returns `308` to `https://learn.chatgpt.com/docs`, which is OpenAI's docs host. Pages were fetched as `https://learn.chatgpt.com/docs/<slug>.md`. Some option tables on the "Command line options" page are built by a script and don't appear in the page Markdown. Those values were read from OpenAI's own combined export, `https://learn.chatgpt.com/docs/llms-full.txt`.
- **Codex repository:** `https://github.com/openai/codex` at commit `ca21fe65` (18 Nov 2026). Facts taken from **source code** rather than docs are marked **[source code, not docs]**.
- **Non-official sources:** a web search returned third-party write-ups of the Codex rollout format (daymade/claude-code-skills, a gist by oneryalcin, and various session-export tools). They were **not used** for any claim below.

---

## Short answer

| | Claude Code | Codex CLI |
|---|---|---|
| Live transcript file on disk | Yes. JSONL, `~/.claude/projects/<project>/<session-id>.jsonl`, saved continuously. Writes are asynchronous and can lag. | Yes. `rollout-*.jsonl` under `~/.codex/sessions`. The docs don't say whether it is written live; the source code appends as it goes. |
| File format a public interface? | **No.** Officially "internal", "changes between versions". | **No.** "isn't a stable interface … may change over time". |
| Official structured stream that works **alongside the interactive TUI** | **Hooks**, configurable per launch with `--settings`: command or HTTP POST to your app. The `MessageDisplay` hook streams assistant text while it renders in the terminal. Also: mods, OpenTelemetry events, the status line, and the SDK `getSessionMessages()` reader. | **App-server protocol.** The TUI itself is an app-server client. Point it at a server you control (`--remote unix://…`) or the shared daemon, and your app can connect to the same server and read or subscribe to the thread. Also: hooks, `notify`, OpenTelemetry. |
| Structured streams that only work **instead of** the TUI | `claude -p --output-format stream-json`, Agent SDK `query()` | `codex exec --json`, TypeScript Codex SDK |
| First-party "view on phone" | Remote Control (Claude app or claude.ai). No third-party API is documented for it. | Codex on mobile in the ChatGPT app, through a connected computer. |

---

## 1. Claude Code

### 1.1 Is a conversation record written to disk while it runs?

**Yes, as JSONL, saved continuously, but it may lag behind.**

- "By default, Claude Code stores transcripts as JSONL at `~/.claude/projects/<project>/<session-id>.jsonl`, where `<project>` is your working directory path with non-alphanumeric characters replaced by `-`." (https://code.claude.com/docs/en/sessions#where-transcripts-are-stored)
- "Sessions are saved continuously to local transcript files as you work" (https://code.claude.com/docs/en/sessions)
- "Each line is a JSON object: a message, a tool use or result, a record of a request Claude Code sent the model, or a metadata entry." (https://code.claude.com/docs/en/sessions#where-transcripts-are-stored)
- About lag, in the `transcript_path` row: "The transcript file is written asynchronously and may lag the in-memory conversation, so it may not yet include the current turn's most recent messages when a hook fires." (https://code.claude.com/docs/en/hooks#common-input-fields)
- At Stop time: "the transcript file isn't guaranteed to include the final message at Stop time on all versions." (https://code.claude.com/docs/en/hooks#stop-input)
- What the file holds: "`projects/<project>/<session>.jsonl` | Conversation transcript: messages, tool calls, and tool results, plus the system prompt, tool definitions, and parameters of the requests Claude Code sent the model" (https://code.claude.com/docs/en/claude-directory#cleaned-up-automatically)
- Subagents get their own files: "`projects/<project>/<session>/subagents/` | Subagent transcripts in the same format as the session transcript" (same URL)
- Retention: the default is 30 days, set with `cleanupPeriodDays`. Writing can be turned off entirely with `CLAUDE_CODE_SKIP_PROMPT_HISTORY` (https://code.claude.com/docs/en/sessions#where-transcripts-are-stored). The file may therefore be missing if the user sets that variable.
- Predictable path: `--session-id` lets you "Use a specific session ID for the conversation (must be a valid UUID)" (https://code.claude.com/docs/en/cli-reference). The launcher can pick the UUID, so it knows the file name before the session starts. That this gives a known file name is an inference from the documented path pattern.
- Two writers corrupt history: "If you resume the same session in two terminals or two `claude -p` runs at the same time, both write to one transcript. When you resume that session later, messages from one of them can be missing." (https://code.claude.com/docs/en/sessions). So never resume the TUI's session from a second process just to read it.

### 1.2 Is the file format stable or public?

**No. It is explicitly internal.**

- "The entry format is internal to Claude Code and changes between versions, so scripts that parse these files directly can break on any release. To build on session data, use `/export` or the script interfaces instead." (https://code.claude.com/docs/en/sessions#where-transcripts-are-stored)
- The status-like files are also not stable: "The files under `~/.claude/jobs/<id>/` are not a stable interface." (https://code.claude.com/docs/en/agent-view#read-session-state-from-a-script)

### 1.3 Official programmatic interfaces that stream structured events

The docs list the supported "script interfaces" at https://code.claude.com/docs/en/sessions#access-conversations-from-scripts:

- "**React to session events**: read the `transcript_path` field that hooks and status line commands receive as input."
- "**Embed Claude in a TypeScript or Python app**: use the Agent SDK to receive each message programmatically."
- `claude -p --output-format json|stream-json`, for a non-interactive run.

#### a) Hooks: work in the same interactive TUI session (best fit)

Hooks run inside the normal interactive session, so the TUI keeps running unchanged.

- **Transport to your app:** "**HTTP hooks** (`type: "http"`): send the event's JSON input as an HTTP POST request to a URL." Also: "For command hooks, this JSON arrives via stdin. For HTTP hooks, it arrives as the POST request body." (https://code.claude.com/docs/en/hooks#common-input-fields)
- **Per-launch configuration:** `--settings` takes a "Path to a settings JSON file or an inline JSON string. Values you set here override the same keys in your `settings.json` files for this session." (https://code.claude.com/docs/en/cli-reference). Separately, "Hook entries merge across settings levels rather than replacing each other" (https://code.claude.com/docs/en/hooks#hook-locations). A plugin can also ship hooks and be loaded for one session with `--plugin-dir` (https://code.claude.com/docs/en/cli-reference).
- **Common fields on every event:** `session_id`, `prompt_id`, `transcript_path`, `cwd`, `hook_event_name`, plus `agent_id` / `agent_type` inside subagents (https://code.claude.com/docs/en/hooks#common-input-fields).

Events useful for a chat view:

| Event | What it gives you | Source |
|---|---|---|
| `UserPromptSubmit` | "receive the `prompt` field containing the submitted text". It also fires on scheduled tasks, background-subagent reports and messages from other sessions. | https://code.claude.com/docs/en/hooks#userpromptsubmit |
| `MessageDisplay` | "Runs while an assistant message streams to the screen … each time a batch of newly completed lines is ready to render, the hook runs once with those lines". Fields: `turn_id`, `message_id`, `index`, `final`, `delta`. "The hook receives assistant message text only". "messages with no text, such as tool-call-only responses, don't trigger it." | https://code.claude.com/docs/en/hooks#messagedisplay |
| `PreToolUse` / `PostToolUse` | `tool_name`, `tool_input`, `tool_use_id`; PostToolUse adds `tool_response` and `duration_ms`. "The exact schema for both depends on the tool." | https://code.claude.com/docs/en/hooks#posttooluse-input |
| `Stop` / `SubagentStop` | "The `last_assistant_message` field contains the text content of Claude's final response, so hooks can access it without parsing the transcript file." | https://code.claude.com/docs/en/hooks#stop-input |
| `Notification` | Matchers include `permission_prompt` ("Claude needs you to approve a tool use … and the prompt has waited about six seconds") and `idle_prompt`. | https://code.claude.com/docs/en/hooks#notification |
| `SessionStart` / `SessionEnd`, `PreCompact` / `PostCompact`, `SubagentStart` | Lifecycle markers | https://code.claude.com/docs/en/hooks#hook-lifecycle |

The docs give these cadences: "per session: `SessionStart` and `SessionEnd`", "per turn: `UserPromptSubmit`, `Stop`, and `StopFailure`", and "on every tool call … `PreToolUse` and `PostToolUse`" (https://code.claude.com/docs/en/hooks#hook-lifecycle). No hook carries a whole structured message object. `MessageDisplay` carries assistant text only.

Cautions:

- **MessageDisplay blocks rendering.** "Claude Code holds each batch until your hook returns, so keep the hook fast." The default timeout is 10 seconds.
- **Async hooks are command-only.** "This field is only available on `type: "command"` hooks." (https://code.claude.com/docs/en/hooks#run-hooks-in-the-background). An HTTP hook endpoint should answer at once: "2xx with an empty body: success".
- **MessageDisplay is display-only.** "The transcript and what Claude sees keep the original text". A hook that returns nothing leaves the terminal unchanged (https://code.claude.com/docs/en/hooks#messagedisplay-output).

#### b) `claude -p --output-format stream-json` and the Agent SDK: instead of the TUI

- `--output-format` is described as "Specify output format for print mode" (https://code.claude.com/docs/en/cli-reference). The related flags `--include-partial-messages`, `--include-hook-events` and `--forward-subagent-text` each "Requires `--print`" or "Requires `--output-format stream-json`".
- "Add the `-p` (or `--print`) flag to any `claude` command to run it non-interactively." (https://code.claude.com/docs/en/headless)
- Agent SDK: "When your code calls `query()`, the SDK spawns a separate `claude` CLI process and talks to it over stdio." (https://code.claude.com/docs/en/agent-sdk/hosting)
- Conclusion: these give a structured stream only for a session the SDK or `-p` drives. **They cannot be attached to an already-running interactive terminal session** (no such option found in official docs).

#### c) Agent SDK session readers: an official parser for the transcript

- "Both SDKs expose functions for enumerating sessions on disk and reading their messages: `listSessions()` and `getSessionMessages()` in TypeScript, `list_sessions()` and `get_session_messages()` in Python. Use them to build custom session pickers, cleanup logic, or transcript viewers." (https://code.claude.com/docs/en/agent-sdk/sessions)
- `getSessionMessages()` "Reads user and assistant messages from a past session transcript". Each result's `message` field is typed `unknown`: "Raw message payload from the transcript". It supports `limit` / `offset` (https://code.claude.com/docs/en/agent-sdk/typescript#getsessionmessages).
- It returns the post-compaction chain: "After auto-compaction, earlier turns are replaced by a summary" (https://code.claude.com/docs/en/agent-sdk/session-storage).
- This is a supported reader of the same `~/.claude/projects` files, so you don't have to parse the internal format yourself. **Whether it is meant to be polled while the session is still running: not found in official docs.** The docs only say "past session".

#### d) Mods: in-process plugin code inside the interactive CLI

- "A mod's handlers are functions that run inside Claude Code" (https://code.claude.com/docs/en/plugins/mods/overview)
- The `session.append` event fires "Once for each row the conversation keeps, such as a prompt, a response block, a tool result, or a notice, before it's stored" (https://code.claude.com/docs/en/plugins/mods/reference#session)
- The `turn.complete` event: "`e.answer` is Claude's final text" (https://code.claude.com/docs/en/plugins/mods/events#follow-a-turn)
- "`$.session` | `messages()` returns the transcript as a list of `{ role, text, toolUses }`". A mod can also call `$.http.fetch` "over `http` or `https`" (https://code.claude.com/docs/en/plugins/mods/api)
- This is the only documented interface that fires on **every stored row**. The cost is that it is plugin code with the user's permissions: "A mod is code that runs with your permissions." (https://code.claude.com/docs/en/plugins/mods/overview)

#### e) OpenTelemetry events: work in interactive sessions, content is opt-in

- `claude_code.user_prompt`: "`prompt`: Prompt content. Redacted by default. Set `OTEL_LOG_USER_PROMPTS=1` to include it"
- `claude_code.assistant_response`: "Logged after each API request that returns text content from the model." `response` is "Redacted to `<REDACTED>` by default. Set `OTEL_LOG_ASSISTANT_RESPONSES=1`"
- `claude_code.tool_result`: includes `tool_name`, `tool_use_id`, and `tool_input` when `OTEL_LOG_TOOL_DETAILS=1`
- Source for all three: https://code.claude.com/docs/en/monitoring-usage#user-prompt-event and https://code.claude.com/docs/en/monitoring-usage#assistant-response-event
- This needs an OTLP receiver in your desktop app. That is heavier than hooks.

### 1.4 Other official things that help

- **Status line input:** the status line script receives `session_id` and "`transcript_path` | Path to conversation transcript file" on stdin (https://code.claude.com/docs/en/statusline#available-data). It re-runs when "A new assistant message arrives", and "Claude Code debounces updates at 300ms" (https://code.claude.com/docs/en/statusline#when-the-status-line-updates). That makes it a cheap way to learn the transcript path and get a nudge per message.
- **Session state:** "`claude agents --json` is the supported way to read session state from outside Claude Code". It gives `status` `busy|waiting|idle` and `waitingFor`, such as `permission prompt` (https://code.claude.com/docs/en/agent-view#read-session-state-from-a-script).
- **First-party phone view:** Remote Control. "A connected device shows the conversation in your terminal as it happens" (https://code.claude.com/docs/en/remote-control#what-connected-devices-see). It goes through Anthropic's servers: "While Remote Control is connected, the session transcript … is stored on Anthropic servers" (https://code.claude.com/docs/en/remote-control#connection-and-security). It requires a claude.ai subscription. **A public API for third-party clients of Remote Control: not found in official docs.**
- **Inbox socket:** the cross-session messaging socket is inbound only ("where other sessions on the machine deliver messages"). It doesn't read the conversation (https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket).

---

## 2. Codex CLI

### 2.1 Is a conversation record written to disk while it runs?

**Yes, as `rollout-*.jsonl` under `~/.codex/sessions`.** The docs don't say whether it is written live. The source code appends items as they are recorded.

- "Session transcripts: `$CODEX_HOME/sessions` (default: `~/.codex/sessions`)" and "Archived sessions: `$CODEX_HOME/archived_sessions`" (https://learn.chatgpt.com/docs/reference/troubleshooting)
- "Codex also saves conversation events in `rollout-*.jsonl` files. These can already live on NFS because one process writes each rollout at a time." (https://learn.chatgpt.com/docs/environments/network-file-systems)
- "Threads are persisted in `~/.codex/sessions`." (https://github.com/openai/codex/blob/main/sdk/typescript/README.md)
- The `codex exec` option `--ephemeral`: "Run without persisting session rollout files to disk." (https://learn.chatgpt.com/docs/non-interactive-mode)
- `transcript_path` in hook input is the "Path to the session transcript file, if any". The example value is `"/workspace/.codex/rollout.jsonl"` (https://learn.chatgpt.com/docs/hooks#common-input-fields and https://learn.chatgpt.com/docs/hooks#sessionend)
- **The docs contradict each other.** One page says "By default, Codex saves local session transcripts under `CODEX_HOME` (for example, `~/.codex/history.jsonl`)" and calls `history.persistence` the setting that controls "whether Codex saves session transcripts to history.jsonl" (https://learn.chatgpt.com/docs/config-file/config-advanced#history-persistence and https://learn.chatgpt.com/docs/config-file/config-reference). Other pages point to `~/.codex/sessions` and `rollout-*.jsonl`. **Which pages are right: not settled in official docs.**
- **Live or at the end: not stated in official docs.**
  - **[source code, not docs]** The `codex-rs/rollout/src/recorder.rs` file says "Persist Codex session rollouts (.jsonl)". Handling `AddItems` calls `state.flush_if_materialized()`, which writes pending items to the file once it exists. New rollouts start with `deferred_creation: true` (the file is created on first persist). (https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
  - **[source code, not docs]** The path is `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<thread-id>.jsonl`. The comment reads "Resolve ~/.codex/sessions/YYYY/MM/DD path" (same file and `rollout_file_name.rs`).
  - **[source code, not docs]** Cold rollouts (`MIN_ROLLOUT_AGE` of 7 days) can be compressed to `.jsonl.zst` (https://github.com/openai/codex/blob/main/codex-rs/rollout/src/compression.rs). The app-server README warns: "Clients sharing this Codex home must support compressed rollout files" (https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md).

### 2.2 Is the file format stable or public?

**No.**

- "`transcript_path` points to a chat transcript for convenience, but the transcript format isn't a stable interface for hooks and may change over time." (https://learn.chatgpt.com/docs/hooks#common-input-fields)
- A schema for rollout lines: not found in official docs.

### 2.3 Official programmatic interfaces that stream structured events

#### a) App-server protocol: can run alongside the interactive TUI (best fit)

- "Codex app-server is the interface Codex uses to power rich clients (for example, the Codex VS Code extension). Use it when you want a deep integration inside your own product: authentication, conversation history, approvals, and streamed agent events." (https://learn.chatgpt.com/docs/app-server)
- **The TUI is itself an app-server client.**
  - "Use `--remote ws://host:port` or `--remote wss://host:port` to connect the TUI to an app server started with `codex app-server --listen ws://IP:PORT`. For a local Unix socket, use `--remote unix://` for the default socket or `--remote unix://PATH` for an explicit path." (https://learn.chatgpt.com/docs/developer-commands?surface=cli)
  - "Remote terminal UI mode lets you run app-server on one machine and connect the Codex CLI terminal interface from another." (https://learn.chatgpt.com/docs/app-server#connect-the-cli-terminal-ui)
- **By default the interactive CLI uses a shared background server.**
  - `--no-daemon`: "Run the interactive CLI without the shared background server. Can't be combined with `--remote` or `codex agents`." (https://learn.chatgpt.com/docs/developer-commands?surface=cli; option table read from llms-full.txt)
  - "In daemon mode, the CLI connects to a background server that stays running between sessions. Its Unix socket works only on the host that started it." (https://learn.chatgpt.com/docs/environments/network-file-systems)
  - `codex agents`: "Open the command center to browse chats and switch between agents on a shared server." (https://learn.chatgpt.com/docs/developer-commands?surface=cli)
  - The daemon backs "remote clients such as the desktop and mobile apps". Also: "If an implicitly discovered daemon cannot initialize the connection, the TUI starts an embedded server instead." (https://github.com/openai/codex/blob/main/codex-rs/app-server-daemon/README.md)
- **Primitives:** "**Thread**: A conversation … **Turn**: A single user request and the agent work that follows … **Item**: A unit of input or output (user message, agent message, command runs, file change, tool call, and more)." (https://learn.chatgpt.com/docs/app-server#core-primitives)
- **Reading without taking over:**
  - "`thread/read` reads stored thread metadata without subscribing to it."
  - "`thread/turns/list` pages through a stored thread's turn history without resuming it."
  - "`thread/items/list` pages through persisted thread items".
  - "`thread/loaded/list` returns the thread IDs currently in memory."
  - Returned threads carry runtime `status` (`notLoaded`, `idle`, `systemError`, or `active` with `activeFlags`, for example `waitingOnApproval`).
  - Source: https://learn.chatgpt.com/docs/app-server#threads
- **Live subscription:**
  - "`thread/start` … automatically subscribes you to turn/item events".
  - "`thread/resume` - reopen an existing thread by id so later `turn/start` calls append to it". It takes `excludeTurns: true`.
  - "`thread/unsubscribe` - unsubscribe this connection … If this was the last subscriber, the server unloads the thread". The phrase "last subscriber" implies several connections can subscribe to one thread.
  - Source: https://learn.chatgpt.com/docs/app-server#api-overview
  - **An explicit statement that a second client can subscribe to a thread another client (the TUI) is running: not found in official docs.**
  - **[source code, not docs]** A comment in `codex-rs/app-server/src/thread_state.rs` reads: "SendThreadResumeResponse is used to resume an already running thread by sending the thread's history to the client and atomically subscribing for new updates."
- **Event stream suitable for a chat view** (https://learn.chatgpt.com/docs/app-server#events):
  - Turn and item lifecycle: `turn/started`, `turn/completed`, `item/started`, `item/completed` ("treat this as the authoritative state").
  - Text deltas: `item/agentMessage/delta` ("appends streamed text for the agent message") and `item/commandExecution/outputDelta`.
  - Item types: `userMessage`, `agentMessage` (`{id, text, phase?}`), `commandExecution`, `fileChange`, `mcpToolCall`, `webSearch`, `reasoning`, `plan`, `contextCompaction`.
  - Status: `thread/status/changed`.
- **Stability caveats:**
  - The `codex app-server` command "is primarily for development and debugging and may change without notice", and its maturity label is "experimental" (https://learn.chatgpt.com/docs/developer-commands?surface=cli).
  - "WebSocket transport is experimental and unsupported." (https://learn.chatgpt.com/docs/app-server#protocol)
  - There is a gated set of experimental methods. Clients that omit `experimentalApi` "stay on the stable API surface" (https://learn.chatgpt.com/docs/app-server#experimental-api-opt-in).
  - "`codex-app-server-daemon` is experimental and its lifecycle contract may change" (https://github.com/openai/codex/blob/main/codex-rs/app-server-daemon/README.md).

Practical reading of the docs, which is an inference: the desktop app starts `codex app-server --listen unix:///path/app.sock` and launches the terminal UI with `codex --remote unix:///path/app.sock`. The app then connects its own client to the same socket. It uses `thread/list` / `thread/loaded/list` to find the thread, `thread/turns/list` / `thread/items/list` for history, and `thread/resume` with `excludeTurns: true` for live `item/*` notifications. Confirm the "second subscriber on a running thread" behaviour by testing.

#### b) `codex exec --json` and the TypeScript SDK: instead of the TUI

- "Non-interactive mode lets you run Codex from scripts … without opening the interactive TUI." With `--json`, "`stdout` becomes a JSON Lines (JSONL) stream so you can capture every event Codex emits while it's running. Event types include `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*`, and `error`." (https://learn.chatgpt.com/docs/non-interactive-mode)
- `codex exec` maturity is "stable" (https://learn.chatgpt.com/docs/developer-commands?surface=cli; table in llms-full.txt). A statement that the JSONL event schema itself is stable: not found in official docs.
- TypeScript SDK: "It spawns the CLI and exchanges JSONL events over stdin/stdout." Its `runStreamed()` "returns an async generator of structured events" (https://github.com/openai/codex/blob/main/sdk/typescript/README.md).
- Python SDK: "The Python SDK controls the local Codex app-server over JSON-RPC." (https://learn.chatgpt.com/docs/codex-sdk)

#### c) Hooks: run in the interactive TUI

- Events: "During a turn | `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `UserPromptSubmit`, `SubagentStop`, `Stop`", plus `SessionStart`, `SessionEnd` and `Interrupt` (https://learn.chatgpt.com/docs/hooks)
- `UserPromptSubmit` gives "`prompt` | User prompt that's about to be sent". `PostToolUse` gives `tool_input` / `tool_response`. `Stop` gives "`last_assistant_message` | Latest assistant message text, if available" (https://learn.chatgpt.com/docs/hooks#userpromptsubmit, #posttooluse, #stop)
- The hook list includes "Send the chat to a custom logging/analytics engine" as an example use (https://learn.chatgpt.com/docs/hooks)
- No per-message or streaming-text hook (equivalent to Claude Code's `MessageDisplay`) was found in official docs. Hosted tools such as WebSearch get no tool hooks (https://learn.chatgpt.com/docs/hooks#tool-coverage).
- **Trust gate:** "Before a non-managed hook can run, Codex requires you to review and trust the exact hook definition." The user does this once in `/hooks`. `--dangerously-bypass-hook-trust` exists "For one-off automation" (https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks)
- Hooks are command or MCP-tool handlers. An HTTP hook type, as Claude Code has: not found in official docs.

### 2.4 Other official things that help

- **`notify` program:** runs on `agent-turn-complete` and "receives a single JSON argument" with `thread-id`, `turn-id`, `cwd`, `input-messages` ("user messages that led to the turn") and `last-assistant-message` (https://learn.chatgpt.com/docs/config-file/config-advanced#notifications)
- **OpenTelemetry:**
  - `codex.user_prompt` carries "length; content redacted unless explicitly enabled".
  - `codex.agent_response` carries "final answers … when `otel.log_agent_responses = true`".
  - `codex.tool_result` carries "duration, success, output snippet".
  - Source: https://learn.chatgpt.com/docs/config-file/config-advanced#observability-and-telemetry
- **`codex queue --thread <SESSION_ID> --message …`:** sends text into an existing session. This helps if the phone should reply later (https://learn.chatgpt.com/docs/developer-commands?surface=cli)
- **First-party phone view:** "Codex on mobile" in the ChatGPT app: "Follow active tasks in Cloud and across connected computers, pick up existing conversations" (https://learn.chatgpt.com/docs/mobile)
- **`/export`:** "Export the conversation as Markdown." This is human-readable, not structured (https://learn.chatgpt.com/docs/developer-commands?surface=cli)

---

## 3. Bottom line for the planned app

1. **Don't build on parsing the raw files as a contract.** Both vendors say the format is internal or unstable.
   - Claude Code: "internal … changes between versions".
   - Codex: "isn't a stable interface … may change over time".
   - Tailing them works today, as long as you accept breakage. Claude Code's file can also lag ("written asynchronously and may lag").
2. **Claude Code:** the documented, terminal-compatible path is **hooks injected per launch** with `--settings '<inline JSON>'`.
   - Use HTTP hooks posting to the desktop app on localhost.
   - Events: `UserPromptSubmit` for the user message, `MessageDisplay` for streaming assistant text, `PreToolUse` / `PostToolUse` for tool rows, `Stop` with `last_assistant_message` for the final text, and `Notification` for "needs approval".
   - For backfill or history, use the SDK's `getSessionMessages()` instead of your own parser.
   - Mods are the alternative if you need every stored row (`session.append`).
3. **Codex:** the documented, terminal-compatible path is **the app-server protocol, shared between the TUI and your app**.
   - The TUI runs with `--remote unix://…` to a server your app controls, or uses the default shared daemon.
   - Your app reads `thread/*` and `item/*` over the same server.
   - It is labelled experimental, and the "second live subscriber" behaviour is only implied in the docs (confirmed in a source comment).
   - Hooks (`UserPromptSubmit`, `PostToolUse`, `Stop`) and `notify` are a simpler but coarser fallback, with no streaming text and a hook-trust prompt.
