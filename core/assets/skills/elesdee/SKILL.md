---
name: elesdee
description: What elesdee is and how this session works. Covers the session branch and worktree, the session tools, and which skill to load for each job.
---

# elesdee sessions

You run inside elesdee, a desktop app for coding agents. The user watches this session in the app and can answer your questions from the desktop or the phone.

## Your workspace

- The session has its own Git branch and its own worktree folder. Work only in this folder.
- Commit your work on the session branch. Do not switch the worktree to another branch.
- The user closes the session. You cannot close it. When your task is done, call `report_done` and stop.

## Session tools

The elesdee MCP server gives you these tools:

| Tool | Use |
| --- | --- |
| `ask_me` | Ask the user a question. |
| `wait_for_answer` | Wait for the answer to a question. |
| `withdraw_question` | Withdraw a question you no longer need. |
| `notify_me` | Send the user a short notification. |
| `report_done` | Mark the task done. |
| `list_sessions` | List the sessions on this computer. |
| `read_session` | Read another session's status and recent output. |
| `send_message` | Send input to another session. |
| `check_usage` | Show the account's remaining usage. |
| `start_session` | Ask the user to start a helper session. |
| `write_handoff_note` | Save the handoff note for the next agent before a client switch, a usage stop or a close. |

The user must approve a new session. Only the user closes a session or switches its provider.

## Skills

- **ask-and-notify**: when to ask, wait, notify and report done.
- **coordinate-sessions**: split work across helper sessions and merge their branches.
- **handoff**: write the handoff note before a client switch, a usage stop or a close.
- **frontend-design**: make deliberate visual choices and avoid template defaults.

Read the skill for the job before you start it. Do not assume a tool exists because the product plan mentions it.
