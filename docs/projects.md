---
title: Projects
description: Group sessions by project and choose the folder they work in.
order: 3
---

# Projects

A project groups sessions that belong together. It has a name and a folder. The folder is the default place where new sessions in the project start.

A new install has no projects. Add one from the welcome screen.

## Add a project

1. Choose **Add project**.
2. Enter a name.
3. Enter the folder path.
4. Choose **Add project**.

The folder must exist on this computer. The app does not create it for you. In the desktop app, choose **Browse…** to pick the folder, or type the path.

## Project settings

Choose the settings button in the project header to set the defaults for new sessions in that project.

- **Default account** fills the account and client in the launch form.
- **Default source branch** fills the source branch. The branch must exist in the project folder's Git repository.

Choose **Clear default account** or **Clear default branch** to remove a default. New sessions then start with no account or branch filled in.

The launch form uses the defaults when you choose the project. You can change the account, client or branch for one session without changing the project defaults. A sample account is never filled in unless you choose it as the project default.

## Sidebar views

The sidebar shows sessions in one of two views.

- **Grouped** puts each session under its project. Choose a project header to collapse or expand it.
- **Flat** shows one list. Each session shows its project name below the session name.

## Filters

- **Show closed sessions** brings closed sessions back into the list. Closed sessions look different from active ones.
- The session filter hides sessions that are not active. In the grouped view, a project with an active session stays open.

## Folder rules

The folder can be:

- A Git repository. Agent sessions create a branch and a worktree from it.
- A plain folder. Terminal sessions can run in it. Agent sessions cannot, because they need Git.

The folder you choose at launch can differ from the project folder. This is useful when a terminal session should work inside an agent session's worktree.

## Next steps

- [Sessions](sessions.md)
- [Agents and providers](agents.md)
