# Agent hooks and permission prompts

The app answers agent permission prompts from the desktop and the phone. The agent still runs in its own terminal; the app learns about a pending action through the agent's hook or event interface and answers it through the same channel.

Version 1 covers Claude Code. Codex uses its app-server and OpenCode 2 its server API; those adapters follow the same session fields and the same public endpoint.

Antigravity is first-class, but it keeps its permission prompts in its own terminal. It gets no hook settings file, it never sets `pendingPermission`, and the app shows no permission card for it. Its session has an empty `shortcuts` list, so the UI shows the base keys.

## Claude Code

The core starts Claude Code with a per-session settings file:

```json
{
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch", "hooks": [{ "type": "http", "url": "http://127.0.0.1:4319/internal/hooks/claude/<sessionId>?token=<token>" }] }],
    "PostToolUse": [{ "matcher": "Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch", "hooks": [{ "type": "http", "url": "..." }] }],
    "Notification": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "Stop": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "MessageDisplay": [{ "hooks": [{ "type": "http", "url": "..." }] }]
  }
}
```

- The token is random per session, kept in the running core and sent only to the agent. It is not written to `state.json`. A request with a wrong or missing token gets 403, as does a foreign Host.
- The hook input is the JSON Claude Code posts. The core reads `hook_event_name`, `session_id`, `cwd`, `tool_name`, `tool_input` and `tool_use_id` where present, and ignores unknown fields.
- `UserPromptSubmit` sets the work state to working and clears the waiting state.
- `PostToolUse` sets waitingFor to null and keeps the session active.
- `MessageDisplay` and `Stop` update the preview with the last non-empty line, capped at about 80 characters; `Stop` sets the work state to done.
- `Notification` with a permission message sets waitingFor to permission.
- The matcher on `PreToolUse` and `PostToolUse` limits the card to mutating actions: `Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch`. Read-only tools, `AskUserQuestion` and `ExitPlanMode` do not reach the hook. If a hook arrives for another tool anyway, the core returns 204 and creates no card; the TUI handles that tool.
- `PreToolUse` for a matching tool creates a pending permission:
  - `requestId`: the `tool_use_id`, or a generated id when absent.
  - `tool`: `tool_name`.
  - `summary`: a short, safe text for the card, such as the command for Bash or the path for Edit.
  - it sets `waitingFor: "permission"`, stores the pending request, emits `session.updated`, and holds the HTTP response.
  - Claude Code waits for the hook. When the user answers, the core returns `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"|"deny","permissionDecisionReason":"..."}}`. A second request while one is held gets `"ask"` instead.
  - If no answer arrives within 10 minutes, the core denies with "No answer from the user. The request expired."

## Session fields

`Session` gains one nullable field:

```ts
interface PendingPermission {
  requestId: string;
  tool: string;
  summary: string;
}

// on Session:
pendingPermission: PendingPermission | null;
```

`waitingFor` is `"permission"` while the field is set. An Antigravity session always has `pendingPermission: null`.

## Public answer endpoint

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| POST | `/api/sessions/{id}/permission` | `{ requestId, decision: "allow" \| "deny" }` | 204 |

- 404 for an unknown session, 409 when there is no pending request or the request id does not match, 409 for a closed session.
- The answer resolves the held hook response. The session's `waitingFor` and `pendingPermission` clear when the agent reports the next event, not before, so a late hook cannot be answered twice.
- The UI shows the card whenever `pendingPermission` is set and sends the answer with this endpoint.

## Rules

- The hook endpoint is local only and is not part of the UI contract. It is named `/internal/hooks/...` so it is clearly separate.
- Tool input may contain secrets. The core stores only the short summary in the session and never logs the raw input. The summary removes control, bidi and zero-width format characters.
- An answer applies to exactly one request id. A second answer for the same id returns 409.
- If the agent exits or the session closes while a request is pending, the core denies it and clears the field.
- If a second `PreToolUse` arrives while one request is held, the core returns `permissionDecision: "ask"` for the second request and leaves the first request held. Claude Code then shows its own terminal prompt for the second tool.
- The settings file is removed when the session closes, the agent exits, the session is deleted, or the core shuts down. A startup sweep removes stale settings files.
- `pendingPermission` and a `waitingFor` value of `permission` are not written to `state.json`. The live session keeps them.

## Local trust

The core service listens on loopback only. The Host and Origin checks stop web pages and foreign hosts. They do not stop other programs on the same machine.

Any program running as the same user can call the local API without an Origin. This includes the permission answer endpoint: a local program can read a pending request from `/api/bootstrap` and post the answer. Code that the agent runs inside its session has the same ability.

The permission card is a convenience and a phone approval path. It is not a security boundary against local code or a prompt-injected agent. The hook token protects the internal hook route. It is not a credential for the public API.
