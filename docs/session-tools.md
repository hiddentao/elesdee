---
title: Session tools
description: How agents ask questions, notify you and work with other sessions.
order: 6
---

# Session tools

Claude Code, Codex and OpenCode 2 sessions get the elesdee tools and the same skill pack: **elesdee**, **Ask and notify**, **Coordinate sessions**, **Handoff** and **Frontend design**. The tools let the agent talk to you and to other sessions without parsing its output.

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

- **Start session** asks you to start another session with a project, account, name, source branch and first prompt. The request appears in the parent session and in the sidebar **Requests** list. Approve or deny it there. Approve with **Allow further sessions from this session** to let later requests from that session start without asking.
- **List sessions** shows the sessions on this computer with their project, status and branch.
- **Read session** shows another session's status and recent output.
- **Send message** sends a line of input to another session.
- **Report done** marks this session's task finished. The session stays active until you close it.

A session started this way shows the session it came from in the sidebar.

## Handoff note

**Write handoff note** saves a short note about the work. The agent writes one before a switch to a different client, a run out of usage, or a close. The app keeps the note and gives it, with the end of the log, to the next agent that takes over the session. The same skill also tells the agent to put the note in the last commit message and push the branch, so the note travels with the branch as well. See [Agents and providers](agents.md) for the skill.

## Usage

**Check usage** shows the usage rows for the session's account.

## Where the tools come from

The core service runs one MCP server. Each session gets a private skill folder under the app's data directory, and the folder is removed when the session closes. Claude Code gets a launch-only MCP config and loads the folder as a plugin. Codex gets the server through a launch config override and reads the skills from a private Codex home for the session. OpenCode 2 gets a launch-only config file that adds the server and the skill folder and merges with your own OpenCode config. None of these changes your repository or your global agent settings.

The **Demo servers** and **Browser checks** skills arrive when those features do. Until then, a session gets the five skills listed above.

## Next steps

- [Permissions](permissions.md)
- [Agents and providers](agents.md)
