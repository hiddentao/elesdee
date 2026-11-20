---
title: Session tools
description: How agents ask questions, notify you and work with other sessions.
order: 6
---

# Session tools

Claude Code, Codex and OpenCode 2 sessions get the elesdee tools and the same skill pack: **elesdee**, **Demo servers**, **Handoff**, **Ask and notify**, **Coordinate sessions**, **Frontend design** and **Browser checks**. The tools let the agent talk to you, work with other sessions, show a page and check a page without parsing its output.

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
- **Wait for session** waits until another session needs you or the agent, finishes, closes or changes state. The wait has a short timeout and returns the session's status. The agent calls it again to keep waiting.
- **Report done** marks this session's task finished. The session stays active until you close it.

A session started this way shows the session it came from in the sidebar.

## Handoff note

**Write handoff note** saves a short note about the work. The agent writes one before a switch to a different client, a run out of usage, or a close. The app keeps the note and gives it, with the end of the log, to the next agent that takes over the session. The same skill also tells the agent to put the note in the last commit message and push the branch, so the note travels with the branch as well. See [Agents and providers](agents.md) for the skill.

## Shared files

**Share file** takes a file or screenshot from the session's folder and adds it to the session. It shows in the **Shared** list at the top of the session's Files panel. Select it to read a text preview, see an image, or download the copy. The copy is kept in the app's data folder, so you can open it after the session closes.

- The path must stay inside the session folder. A path outside it, a folder, a link that leaves the folder, or a file above 10 MiB is refused.
- Secret files such as `.env` and private keys are refused. A file that contains a stored account key is refused too.
- The original file is not changed and the app stores no key in the copy.

## Demo servers

**Register demo server** tells the app that a demo server runs on a loopback port. The agent gives a short name and the port, and the tool returns the address to print, such as `http://localhost:3000`. Start the server before you register its port.

The session's Files panel lists each registered server with its address and state:

- **Active** servers show an **Open** link and a **Stop** button. Open the address on this computer, or the same port on a paired phone.
- **Stop** removes the server from the phone's list and closes its tunnels. The process is not the app's to stop; the agent stops it.
- Close and pause stop every server in the session. Register the port again after you resume.

Limits and rules:

- A session holds at most 8 demo servers.
- A port is 1024–65535. Privileged ports and the elesdee service port are refused.
- A port belongs to one active session at a time. Two sessions never share one.
- Only an active registered port can be forwarded to a phone. Another port is refused.
- The record is kept with the session. After an app restart it shows as stopped, because the server no longer runs.

The optional Tailscale address is not built yet.

## Browser

Each session has its own browser. It has separate tabs, cookies, storage and logins from every other session and from your personal browser. The agent uses these tools:

- **Open page** opens an http or https address and returns its title and visible text.
- **Read page** returns the visible text of the open page.
- **Click** selects an element by CSS selector and clicks it.
- **Type** focuses an element and types text into it.
- **Wait** returns when a selector or text appears, or when a short timeout ends.
- **Take screenshot** captures the page and shares the image with you.

The agent can work through a form or check a UI change and then show you the result. The **Browser** tab of the session panel shows the same tabs and a live picture of the page. The tools are limited to the calling session, so an agent cannot read or control another session's tabs. The browser starts on first use. Closing or pausing the session stops the browser and keeps its logins.

The browser uses an installed Chrome or Chromium. See [Sessions](sessions.md) for the panel controls.

## Usage

**Check usage** shows the usage rows for the session's account.

## Where the tools come from

The core service runs one MCP server. Each session gets a private skill folder under the app's data directory, and the folder is removed when the session closes. Claude Code gets a launch-only MCP config and loads the folder as a plugin. Codex gets the server through a launch config override and reads the skills from a private Codex home for the session. OpenCode 2 gets a launch-only config file that adds the server and the skill folder and merges with your own OpenCode config. None of these changes your repository or your global agent settings.

A session gets the seven skills listed above.

## Next steps

- [Permissions](permissions.md)
- [Agents and providers](agents.md)
