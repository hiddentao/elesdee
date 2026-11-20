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
2. Choose a project.
3. Choose **Agent** or **Terminal**.
4. For an agent session, choose a client and an account.
5. Set the folder if you want a different one.
6. Optionally enter a name.
7. Choose **Start**.

If you leave the name empty, an agent session is named after its client and a terminal session after its folder.

At most 64 sessions can run at once. Close a session to free a slot.

## Terminals

The terminal is the real agent terminal. Type in it, scroll it and use its keyboard shortcuts. The app sends your input to the program, not to a copy.

Each agent has its own short key list in the session. The core service supplies those shortcuts, so they follow the selected agent.

## Tabs and the sidebar

Choose a session in the sidebar to open it in a tab. Each project keeps its own tab strip, and the app restores the last active tab when you return to the project.

A tab is a view, not the session itself. The session keeps running when you move to another tab.

The sidebar shows two states:

- **Active**, with a short preview of recent output, or **Waiting for you** when the session needs an answer.
- **Closed**, when **Show closed sessions** is on.

## Rename

Open the session menu and choose **Rename…**. The session name changes. The Git branch keeps its original name.

## Close

Open the session menu and choose **Close…**. Confirm in the dialog.

For an agent session, close does this:

1. Commits any uncommitted work in the worktree.
2. Pushes the branch when the repository has a remote.
3. Removes the worktree folder.
4. Keeps the branch.

If a step fails, the worktree stays and the reason appears in the session preview. The branch stays in all cases.

For a terminal session, close stops the terminal. The folder stays as it is.

Closing a session ends its process. The session moves to the closed list.

## Reopen

Turn on **Show closed sessions**, open the session menu and choose **Reopen**.

The app opens the session in a new tab, restores its log, and starts the agent again. A Git session gets its worktree back from the saved branch.

## Delete

A closed session can be deleted. Open the session menu and choose **Delete…**.

Delete removes the session and its log from the app. The branch stays in Git.

## Session menu

Right-click a session in the sidebar or its tab, or use the menu button in the session header. The menu holds:

- **Reopen** for a closed session
- **Rename…**
- **Copy folder path**
- **Copy branch name**
- **Close…** for an active session, or **Delete…** for a closed one

The branch chip in the session header has a separate menu with **Copy branch name**, **Copy folder path**, **Open folder** and **Open in Terminal**.

## Next steps

- [Agents and providers](agents.md)
- [Permissions](permissions.md)
