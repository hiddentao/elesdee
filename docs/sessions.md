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
2. Choose a project. Its default account, client and source branch fill in when the project has them. See [Projects](projects.md).
3. Choose **Agent** or **Terminal**.
4. For an agent session, choose a client and a preset. Every connected account is a preset. A stored preset also brings its model, environment variables and config settings into this launch. You can change the filled-in default. The closed field keeps the account's remaining usage visible.
5. Choose the accounts the session may use. The accounts of the starting client are selected by default. Add accounts of other clients, or remove any account except the one in use. The app switches automatically only between these accounts.
6. Set the folder if you want a different one.
7. For an agent session, optionally choose a source branch. The app fetches the remote first, so the new session branch starts from the latest commit. When the branch tracks a remote branch, the app also moves that local branch forward when it is not checked out. It never changes your own checkout and never forces a branch. With no choice, the session starts from the current branch.
8. Optionally enter a first prompt. The app passes it to the agent at launch.
9. Optionally enter a name.
10. Open **Additional settings** to add environment variables for this session. Use one `NAME=value` per line. When the repository has uncommitted changes, it also asks whether to copy them into the new session or leave them out. The choice applies to this launch only.
11. Choose **Start**.

If you leave the name empty, an agent session is named from the first prompt, or after its client when there is no prompt. A terminal session is named after its folder.

At most 64 sessions can run at once. Close a session to free a slot.

## Terminals

The terminal is the real agent terminal. Type in it, scroll it and use its keyboard shortcuts. The app sends your input to the program, not to a copy.

Each agent has its own short key list in the session. The core service supplies those shortcuts, so they follow the selected agent.

## Files and Changes

Each session has a side panel. It is closed by default and remembers its open state and tab for each session. Open it with the panel button in the session header. The terminal stays usable while the panel is open.

**Files** shows the folders and files in the session's worktree or folder. Select a file to open it in the editor. The editor highlights the language, shows a dot for unsaved changes, and saves only when you choose **Save**. A failed read or save shows an error in the panel.

Files must be UTF-8 text up to 1 MiB. The app refuses binary files and larger files. Every path stays inside the session folder: paths that climb out, leave through a symlink, or name a folder where a file is expected are refused.

**Changes** shows the files that differ from the branch point for an agent session, committed or not. A terminal session shows the changes in its working tree against the current commit. Each file shows added and removed line counts, and selecting it shows a diff. In a plain folder, the panel says the folder is not a Git repository.

## Tabs and the sidebar

Choose a session in the sidebar to open it in a tab. Each project keeps its own tab strip, and the app restores the last active tab when you return to the project.

A tab is a view, not the session itself. The session keeps running when you move to another tab.

The sidebar shows two states:

- **Active**, with a short preview of recent output, or **Waiting for you** when the session needs an answer.
- **Closed**, when **Show closed sessions** is on.

## Questions

An agent can ask you a question through the session tools. The question appears as a card above its terminal and in the sidebar **Questions** list. Answer it there. The session stays marked as waiting until you answer, the agent withdraws the question, or you close the session.

See [Session tools](session-tools.md).

## Session requests

A session can ask to start another session. The request appears in the parent session and in the sidebar **Requests** list. Approve it to start the new session, or deny it. With **Allow further sessions from this session**, later requests from that session start without asking. A session started this way shows where it came from in the sidebar.

## Switch account

Click the provider pill in the session header. The chooser lists the accounts allowed for this session with their remaining usage and marks the account the automatic switch would use next. Choose an account and confirm. The session keeps its worktree and restarts with the new account. The same client continues its conversation. A different client starts again in the same worktree, and it gets the handoff note and the end of the previous log as its first message.

The chooser also edits the allowed list while the session runs. Add an account from the filterable list, or remove one. The account in use always stays allowed. A switch to an account outside the list is refused, and the app never switches to one by itself.

You can also open the session menu and choose **Switch account…**.

Hover the usage bar in the pill for the current account's limits and the session's context left. The context left comes from Claude Code. Other clients show **Context left unknown**. Context compacts automatically, and the app shows no context warning.

The app switches by itself when an account is near its usage limit and an allowed account has room. It uses the latest usage it read from the provider. If a read fails, the last numbers stay visible and the account says the reading is stale; the failure does not stop a session. Subscription accounts come first, then pay-per-use accounts; within each group it takes the account with the most room, and a tie keeps the current client. If no allowed account has room, the session pauses. A pause keeps the handoff note, and a switch to a different client passes it on. Choose **Resume** when an allowed account has room, or wait for the app to resume it.

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

## Quit

When you quit while sessions run, the app asks first. The dialog shows how many sessions are still running. Cancel keeps working. Quit stops the sessions and closes the app. On the next launch the sessions restart in place and continue their conversations. You do not need to do anything.

## Restart recovery

If the app or this computer restarts, the sessions that were running restart when elesdee next starts. Each one reopens in the same worktree with the same account and client, and the agent continues its own conversation.

A terminal session opens a new shell in the same folder, with the earlier output above it.

If an agent has no saved conversation ID, it continues its most recent conversation and the session preview says so.

If a session's folder, account or client is missing, that session stays paused with the reason. The other sessions still restart.

## Delete

A closed session can be deleted. Open the session menu and choose **Delete…**.

Delete removes the session and its log from the app. The branch stays in Git.

## Session menu

Right-click a session in the sidebar or its tab, or use the menu button in the session header. The menu holds:

- **Reopen** for a closed session
- **Rename…**
- **Switch account…** for an active agent session
- **Copy folder path**
- **Copy branch name**
- **Close…** for an active session, or **Delete…** for a closed one

The branch chip in the session header has a separate menu with **Copy branch name**, **Copy folder path**, **Open folder** and **Open in Terminal**.

## Next steps

- [Agents and providers](agents.md)
- [Permissions](permissions.md)
