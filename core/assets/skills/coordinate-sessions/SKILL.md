---
name: coordinate-sessions
description: Split a large task across helper sessions, write clear briefs, track their progress, and merge their branches into the session branch.
---

# Coordinate sessions

Use helper sessions when the work divides into independent parts that can run at the same time without editing the same files.

## Plan the split

- Give each helper one outcome. Do not give two helpers the same files or the same decision.
- Keep integration and review in this session.
- Prefer the smallest number of helpers that shortens the work. Do not start helpers for small tasks.

## Write the brief

`start_session` starts a helper. It needs `projectId`. Optional fields are `accountId`, `name`, `firstPrompt` and `sourceBranch`.

- `accountId` defaults to this session's account.
- Always pass a first prompt. Include:
  - the goal, in one or two sentences;
  - the files or areas the helper may change;
  - what it must not change;
  - the acceptance check, such as a command or a test;
  - the branch name to use, when the task depends on a branch.
- Tell the helper to commit its work and to call `report_done`.

The user must approve each new session, unless the user allowed further sessions from this session.

## Track progress

- `list_sessions` returns each session's project, status, work state and branch.
- `read_session` returns one session's status, waiting state and recent output preview.
- `send_message` sends a line of input to a helper, for example an answer or a correction.
- `wait_for_session` waits for a helper to need input, finish or change state. It returns at its timeout with the status, so call it again to keep waiting.
- Check the helpers at sensible intervals. Do not poll in a tight loop.
- If a helper is waiting for input, use `read_session` to see why and `send_message` to unblock it.
- A helper can share a screenshot or a file with `share_file`. It appears in that session's Files panel.

## Merge the results

When a helper reports done:

1. Read its branch from `list_sessions`.
2. Merge the branch into this session's branch: `git merge <branch>`.
3. Resolve conflicts in this session. Do not ask a helper to resolve a conflict in the coordinator's files.
4. Run the project checks after each merge.
5. Keep the helper branches. The user decides when to delete them.

Do not use force-push. Do not merge a helper's branch into the project's main branch; this session's branch is the integration point.
