---
title: Sessions
description: Start, watch and close agent and terminal sessions.
order: 4
---

# Sessions

A session is one terminal doing work. It is either active or closed. A session stays active while the agent works, waits for you, pauses or reports done. Only you close it.

## Session types

**Agent session**

Runs Claude Code, Codex, OpenCode 2 or Antigravity. It needs a Git repository. The app creates a new branch and a worktree for the session, then starts the agent inside that worktree.

The branch name uses the form `elesdee/<project>/<name>`. If the name is taken, the app adds a number.

**Terminal session**

Runs a plain shell. It has no agent, branch or worktree. The folder can be a Git repository or a plain folder.

## Start a session

1. Choose **New session**.
2. Choose a project. Its default AI config and source branch fill in when the project has them. See [Projects](projects.md).
3. Choose **Agent** or **Terminal**.
4. For an agent session, choose an AI config. The config brings its provider, client, model, effort, extra arguments, environment variables and config settings into this launch. You can change the filled-in default. The closed field keeps the provider's remaining usage visible.
5. Choose the AI configs the session may use. The configs of the starting client are selected by default. Add configs of other clients, or remove any config except the one in use. The app switches automatically only between these configs.
6. Set the repository folder if you want a different one. The list shows the repositories earlier sessions started from, even after those sessions moved to another project; pick one, or type a path. When GitHub is connected, type in the same box to search your GitHub repositories. A repository that a project already holds is marked **Added** and opens that project's folder. The match uses the git remote, so two checkouts with the same folder name stay apart. A repository that is not on this computer is cloned into the clone folder you choose, and the session starts in the cloned folder.
7. For an agent session, optionally choose a source branch. The app fetches the remote first, so the new session branch starts from the latest commit. When the branch tracks a remote branch, the app also moves that local branch forward when it is not checked out. It never changes your own checkout and never forces a branch. With no choice, the session starts from the current branch.
8. Optionally enter a name.
9. Open **Additional settings** to enter the first prompt, add environment variables for this session, and choose what to do with uncommitted changes. Use one `NAME=value` per line for the variables. When the repository has uncommitted changes, it asks whether to copy them into the new session or leave them out. The choice applies to this launch only.
10. Choose **Start**.

If you leave the name empty, an agent session is named from the first prompt, or after its client when there is no prompt. A terminal session is named after its folder.

At most 64 sessions can run at once. Close a session to free a slot.

## Move a session to another project

A project groups sessions. Moving a session changes only its project. The session keeps its folder, branch, worktree and log. A running session keeps running, and a closed session keeps its log.

In the grouped session list, drag a session onto another project. The target project shows a dashed outline. Press Escape during the drag to cancel. A small pointer movement still selects the session.

You can also open the session menu and choose **Move to project…**. Pick a project from the filterable list and confirm. This is the same move as dragging.

To move a session with the keyboard, focus its row and press **M**. The sidebar says which project is the target. Use the up and down arrows to choose another project, **Enter** to move, or **Escape** to cancel.

## Terminals

The terminal is the real agent terminal. Type in it, scroll it and use its keyboard shortcuts. The app sends your input to the program, not to a copy.

Each agent has its own short key list in the session. The core service supplies those shortcuts, so they follow the selected agent.

## Files, Changes and the browser

Each session has a side panel. It is closed by default and remembers its open state and tab for each session. Open it with the panel button in the session header. The terminal stays usable while the panel is open.

**Files** shows the folders and files in the session's worktree or folder. Select a file to open it in the editor. The editor highlights the language, shows a dot for unsaved changes, and saves only when you choose **Save**. A failed read or save shows an error in the panel.

Files must be UTF-8 text up to 1 MiB. The app refuses binary files and larger files. Every path stays inside the session folder: paths that climb out, leave through a symlink, or name a folder where a file is expected are refused.

**Changes** shows the files that differ from the branch point for an agent session, committed or not. A terminal session shows the changes in its working tree against the current commit. Each file shows added and removed line counts, and selecting it shows a diff. In a plain folder, the panel says the folder is not a Git repository.

**Browser** is this session's own browser. It has its own tabs, cookies, storage and logins, separate from every other session and from your personal browser. Type an address and press Enter, or choose **Open**, to open a page. Use **Back**, **Forward** and **Reload** for the active tab, and the tab strip to switch, add or close tabs. The panel shows a loading state while a page opens, then the latest picture of the page. Click or type on the picture to use the page itself. Key combinations such as select all, copy, cut, paste and undo also run in the page. A page that handles its own find receives that key; the panel has no separate find bar. Your clicks and keys go to this session's browser only; the shortcuts that control the app window stay with the app.

The app uses Chrome or Chromium when it is installed. Otherwise the first use of a session browser downloads a pinned Chrome for Testing build. The panel shows the download step and the progress; if it fails, it shows the reason and a **Retry** button.

The browser starts only when the session needs it. Closing or pausing the session stops the browser and closes its tabs, but keeps the session's logins for the next start. A closed session cannot use its browser.

An agent can also use the session browser through its tools: open a page, read the visible text, click, type, wait for something to appear, and share a screenshot with you. The agent can read the browser's recent console messages and network requests to find a failing request or a page error. Those lists are short, and they hide login details, sensitive address values and the keys the app knows. A screenshot appears with the session's shared files and on the phone. A session cannot see or control another session's tabs.

## Tabs and the sidebar

Choose a session in the sidebar to open it in a tab. Each project keeps its own tab strip, and the app restores the last active tab when you return to the project. The plus after the last tab starts a new session in that project.

A tab is a view, not the session itself. The session keeps running when you move to another tab.

The sidebar shows two states:

- **Active**, with a short preview of recent output, or **Waiting for you** when the session needs an answer.
- **Closed**, when **Show closed sessions** is on.

## Questions

An agent can ask you a question through the session tools. The question appears as a card above its terminal and in the sidebar **Questions** list. Answer it there. The session stays marked as waiting until you answer, the agent withdraws the question, or you close the session.

See [Session tools](session-tools.md).

## Session requests

A session can ask to start another session. The request appears in the parent session and in the sidebar **Requests** list. Approve it to start the new session, or deny it. With **Allow further sessions from this session**, later requests from that session start without asking. A session started this way shows where it came from in the sidebar.

## Switch AI config

Click the provider pill in the session header. The chooser lists the AI configs allowed for this session with their provider usage and marks the config the automatic switch would use next. Choose a config and confirm. The session keeps its worktree and restarts with the new config. The same client continues its conversation. A different client starts again in the same worktree, and it gets the handoff note and the end of the previous log as its first message.

The chooser also edits the allowed list while the session runs. Add a config from the filterable list, or remove one. The config in use always stays allowed. A switch to a config outside the list is refused, and the app never switches to one by itself.

You can also open the session menu and choose **Switch AI config…**.

Hover the usage bar in the pill for the current provider's limits and the session's context left. The context left comes from Claude Code. Other clients show **Context left unknown**. Context compacts automatically, and the app shows no context warning.

The app switches by itself when a provider is near its usage limit and an allowed config has room. A prepaid provider is near its limit when its balance is at or below the minimum balance you set; a balance above it has room. It uses the latest usage it read from the provider. If a read fails, the last numbers stay visible and the provider says the reading is stale; the failure does not stop a session. Subscription providers come first, then pay-per-use providers; within each group it takes the provider with the most room, and a tie keeps the current client. If no allowed config has room, the app asks the agent to commit its work, write the handoff note and push the branch. It then pauses the session. If the agent does not finish, the app commits and pushes what it can. A failed commit or push keeps the worktree, and the reason appears in the paused session. A pause keeps the handoff note, and a switch to a different client passes it on. Choose **Resume** at any time, or wait for the app to resume the session when an allowed config has room.

An agent can also report that it hit a provider limit. The app acts on the report at once, without waiting for the next usage read.

## Rename

Open the session menu and choose **Rename…**. The session name changes. The Git branch keeps its original name.

## Close

Open the session menu and choose **Close…**. Confirm in the dialog.

For an agent session, close does this:

1. Commits any uncommitted work in the worktree. If the agent saved a handoff note, the commit message carries it.
2. Pushes the branch when the repository has a remote.
3. Removes the worktree folder.
4. Keeps the branch.

A clean worktree keeps its last commit. When that commit does not carry the handoff note, the app adds an empty commit for the note. The note is never written as a file in the repository.

If a step fails, the worktree stays and the reason appears in the session preview. The branch stays in all cases.

For a terminal session, close stops the terminal. The folder stays as it is.

Closing a session ends its process. The session moves to the closed list.

## Reopen

Turn on **Show closed sessions**, open the session menu and choose **Reopen**.

The app opens the session in a new tab, restores its log, and starts the agent again. A Git session gets its worktree back from the saved branch. The app resumes the agent's saved conversation when it has one; otherwise it continues the most recent conversation in that folder. When the agent cannot resume its conversation, it also gets the handoff note and the end of the log.

## Close the window

Closing the window does not quit. The window hides, and elesdee keeps running with every session. A paired phone stays connected. Click the Dock icon or open elesdee again to show the same tabs.

## Quit

When you quit with Command-Q or **Quit elesdee** from the app menu while sessions run, the app asks first. The dialog shows how many sessions are still running. Cancel keeps working. Quit stops the sessions and closes the app. On the next launch the sessions restart in place and continue their conversations. You do not need to do anything.

## Restart recovery

If the app or this computer restarts, the sessions that were running restart when elesdee next starts. Each one reopens in the same worktree with the same AI config and client, and the agent continues its own conversation.

A terminal session opens a new shell in the same folder, with the earlier output above it.

If an agent has no saved conversation ID, it continues its most recent conversation and the session preview says so.

If a session's folder, AI config or client is missing, that session stays paused with the reason. The other sessions still restart.

## Delete

A closed session can be deleted. Open the session menu and choose **Delete…**.

Delete removes the session and its log from the app. The branch stays in Git.

## Session menu

Right-click a session in the sidebar or its tab, or use the menu button in the session header. The menu holds:

- **Reopen** for a closed session
- **Rename…**
- **Move to project…** when another project exists
- **Switch AI config…** for an active agent session
- **Close…** for an active session, or **Delete…** for a closed one

The branch chip in the session header has the only copy actions: **Copy branch name**, **Copy folder path**, **Open folder** and **Open in Terminal**.

## Next steps

- [Agents and providers](agents.md)
- [Permissions](permissions.md)
