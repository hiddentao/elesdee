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

The **Notify me** tool sends a short message. The app shows a desktop notification and a message in the open window. **Report done** also sends a desktop notification.

The app also sends a desktop notification when a session waits for your input, a permission answer or a new-session approval. Click the notification to bring the app forward and open that session. When the app window is already in front on that session, the card there is enough, so the app does not send the notification.

## Session coordination

- **Start session** asks you to start another session with a project, AI config, name, source branch and first prompt. The request appears in the parent session and in the sidebar **Requests** list. Approve or deny it there. Approve with **Allow further sessions from this session** to let later requests from that session start without asking.
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
- Secret files such as `.env` and private keys are refused. A file that contains a stored provider key is refused too.
- The original file is not changed and the app stores no key in the copy.

## Demo servers

**Register demo server** tells the app that a demo server runs on a loopback port. The agent gives a short name and the port, and the tool returns the address to print, such as `http://localhost:3000`. When Tailscale is installed and running, the tool also returns a tailnet address, such as `https://machine.tailnet.ts.net:3000` or `http://100.x.y.z:3000`, for devices that are on the tailnet but do not have the app. Start the server before you register its port.

The session's Files panel lists each registered server with its address and state:

- **Active** servers show an **Open** link and a **Stop** button. Open the address on this computer, or the same port on a paired phone. When the server has a tailnet address, the panel shows and opens that too.
- The panel opens only a loopback address or a tailnet address. It never opens another address.
- **Stop** removes the server from the phone's list and closes its tunnels. The process is not the app's to stop; the agent stops it.
- Close and pause stop every server in the session. Register the port again after you resume.

Tailscale is optional. Without a running Tailscale client, a server has only its local address and the phone forwarding. A failed read never blocks a registration.

Limits and rules:

- A session holds at most 8 demo servers.
- A port is 1024–65535. Privileged ports and the elesdee service port are refused.
- A port belongs to one active session at a time. Two sessions never share one.
- Only an active registered port can be forwarded to a phone. Another port is refused.
- The record is kept with the session. After an app restart it shows as stopped, because the server no longer runs.

## Making a demo server public

The session's Files panel has a **Make public** action for an active server. It asks first and says that anyone on the internet can open the server. The confirmation holds focus and pauses the rest of the window until you answer. Choose **Make public** to confirm. The panel then shows the public HTTPS address, such as `https://machine.tailnet.ts.net/`, with one **Stop public** action.

- This needs Tailscale, installed, running and signed in, and Funnel allowed for the tailnet. Without it the server stays local and the panel shows the reason.
- Only one demo server is public at a time. Making another server public turns the first one off.
- **Stop public**, closing the session, pausing the session and deleting a closed session turn it off. An app restart also turns off an address that was left on.
- If Tailscale refuses to turn the address off, the app keeps the address on the record, because the address may still be public. If the server is active, the panel shows the reason and **Stop public** can be retried. After a close or pause, the server shows as stopped, and the panel still shows the retained address with a warning and one **Stop public** action; that action retries the turn-off and shows the reason if Tailscale refuses again. The next app start also retries.
- Delete tries the turn-off before it removes anything. If the address is still on, the app refuses the delete and keeps the session and its address, so you can choose **Delete** again after Tailscale accepts the command.
- The app uses the Tailscale client it found. It never exposes another port and never takes an address from the agent.

## Browser

Each session has its own browser. It has separate tabs, cookies, storage and logins from every other session and from your personal browser. The agent uses these tools:

- **Open page** opens an http or https address and returns its title and visible text.
- **Read page** returns the visible text of the open page.
- **Click** selects an element by CSS selector and clicks it.
- **Type** focuses an element and types text into it.
- **Wait** returns when a selector or text appears, or when a short timeout ends.
- **Take screenshot** captures the page and shares the image with you.

The agent can work through a form or check a UI change and then show you the result. The **Browser** tab of the session panel shows the same tabs and a live picture of the page. The tools are limited to the calling session, so an agent cannot read or control another session's tabs. The browser starts on first use. Closing or pausing the session stops the browser and keeps its logins.

The browser uses an installed Chrome or Chromium when one exists. Otherwise the app downloads a pinned Chrome for Testing build on first use and shows the progress in the **Browser** tab. See [Sessions](sessions.md) for the panel controls.

## Usage

**Check usage** shows the usage rows for the session's provider.

**Report limit** tells the app that the provider hit a usage limit. The app switches the session to an allowed config with room or pauses it at once, instead of waiting for the next usage read. Use it when a request is refused because of a limit. A pause asks the agent to commit, write the handoff note and push before it stops; the app commits and pushes any leftover work and keeps the worktree.

## Where the tools come from

elesdee provides the same tools and skills to Claude Code, Codex and OpenCode 2 sessions. Each session gets a private skill folder, and the folder is removed when the session closes. None of these changes your repository or your global agent settings.

A session gets the seven skills listed above.

## Next steps

- [Permissions](permissions.md)
- [Agents and providers](agents.md)
