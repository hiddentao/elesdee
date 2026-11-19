# Agent hooks and permission prompts

The app answers agent permission prompts from the desktop and the phone. The agent still runs in its own terminal; the app learns about a pending action through the agent's hook or event interface and answers it through the same channel.

Version 1 covers Claude Code. Codex uses its app-server and OpenCode 2 its server API; those adapters follow the same session fields and the same public endpoint.

## Claude Code

The core starts Claude Code with a per-session settings file:

```json
{
  "hooks": {
    "PreToolUse": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:4319/internal/hooks/claude/<sessionId>?token=<token>" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "Notification": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "Stop": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "http", "url": "..." }] }],
    "MessageDisplay": [{ "hooks": [{ "type": "http", "url": "..." }] }]
  }
}
```

- The token is random per session, stored in state and sent only to the agent. A request with a wrong or missing token gets 403, as does a foreign Host.
- The hook input is the JSON Claude Code posts. The core reads `hook_event_name`, `session_id`, `cwd`, `tool_name`, `tool_input` and `tool_use_id` where present, and ignores unknown fields.
- `UserPromptSubmit` sets the work state to working and clears the waiting state.
- `PostToolUse` sets waitingFor to null and keeps the session active.
- `MessageDisplay` and `Stop` update the preview with the last non-empty line, capped at about 80 characters; `Stop` sets the work state to done.
- `Notification` with a permission message sets waitingFor to permission.
- `PreToolUse` creates a pending permission:
  - `requestId`: the `tool_use_id`, or a generated id when absent.
  - `tool`: `tool_name`.
  - `summary`: a short, safe text for the card, such as the command for Bash or the path for Edit.
  - it sets `waitingFor: "permission"`, stores the pending request, emits `session.updated`, and holds the HTTP response.
  - Claude Code waits for the hook. When the user answers, the core returns `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"|"deny","permissionDecisionReason":"..."}}`.
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

`waitingFor` is `"permission"` while the field is set.

## Public answer endpoint

| Method | Path | Body | Result |
| --- | --- | --- | --- |
| POST | `/api/sessions/{id}/permission` | `{ requestId, decision: "allow" \| "deny" }` | 204 |

- 404 for an unknown session, 409 when there is no pending request or the request id does not match, 409 for a closed session.
- The answer resolves the held hook response. The session's `waitingFor` and `pendingPermission` clear when the agent reports the next event, not before, so a late hook cannot be answered twice.
- The UI shows the card whenever `pendingPermission` is set and sends the answer with this endpoint.

## Rules

- The hook endpoint is local only and is not part of the UI contract. It is named `/internal/hooks/...` so it is clearly separate.
- Tool input may contain secrets. The core stores only the short summary in the session and never logs the raw input.
- An answer applies to exactly one request id. A second answer for the same id returns 409.
- If the agent exits or the session closes while a request is pending, the core denies it and clears the field.
