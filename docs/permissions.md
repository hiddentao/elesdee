---
title: Permissions
description: Answer Claude Code permission requests from the app.
order: 7
---

# Permissions

Claude Code, Codex and OpenCode 2 send approval requests to the app. The request appears as a card in the session view. Antigravity keeps its prompts in its own terminal.

## What appears on a card

A card shows the tool, a summary of the request and the reason. The session waits until you answer.

- **Allow** sends the request to Claude Code.
- **Deny** refuses it.

The sidebar marks the session as **Waiting for you** until you answer. A pending request also survives a window reopen, so an answer is not lost when you switch views.

## Covered tools

The card covers tools that can change your computer:

`Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`

Read-only tools, `AskUserQuestion` and `ExitPlanMode` stay in the terminal interface. Answer those there.

## Which clients use cards

Claude Code, Codex and OpenCode 2 send permission requests to the app. Antigravity always keeps its prompts in its terminal.

Codex connects through its app-server websocket. OpenCode 2 exposes its permission list from its session server. Answering on the card answers the agent in both cases.

## What the card does not protect

The app trusts the user account that runs it. Another program running as the same user can call the local service, including the permission routes. The card helps you answer requests in one place. It is not a security boundary against local code.

The card also cannot stop an agent that has been tricked by a prompt injection from answering its own request. Treat the card as a review step, not as a sandbox.

## Next steps

- [Agents and providers](agents.md)
- [Troubleshooting](troubleshooting.md)
