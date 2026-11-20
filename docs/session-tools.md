---
title: Session tools
description: How agents ask questions, notify you and work with other sessions.
order: 6
---

# Session tools

Claude Code sessions get the elesdee tools and a short skill. The tools let the agent talk to you and to other sessions without parsing its output.

The tools are local to this computer. They use the core service and the same trust as the rest of the app.

## Questions

The **Ask me** tool records a question for you. The question appears:

- in the session, as a card above the terminal;
- in the sidebar, as **Questions** with a count;
- in the Questions list, with every pending question from every session.

Choose an answer or type one. The answer goes back to the agent. The question stays pending until you answer it, the agent withdraws it, or the session closes.

Use **Wait for answer** in the agent when it needs your answer. The agent can also keep working and collect the answer later.

## Notifications

The **Notify me** tool sends a short message. The app shows it as a notification.

## Session coordination

- **List sessions** shows the sessions on this computer with their project, status and branch.
- **Read session** shows another session's status and recent output.
- **Send message** sends a line of input to another session.
- **Report done** marks this session's task finished. The session stays active until you close it.

## Usage

**Check usage** shows the usage rows for the session's account.

## Where the tools come from

The core service runs one MCP server. Each Claude Code session gets its own address on that server and a launch-only MCP config. The app also writes a small skill plugin and passes it to Claude Code for that launch. Neither file changes your repository or your global agent settings.

The same tools for Codex and OpenCode 2 are not built yet.

## Next steps

- [Permissions](permissions.md)
- [Agents and providers](agents.md)
