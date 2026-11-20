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
2. Choose **Local folder** or **From GitHub**.
3. For a local folder, enter a name and the folder path.
4. For GitHub, choose a repository and a clone folder.
5. Choose **Add project**, or **Clone and add project** for GitHub.

The local folder must exist on this computer. The app does not create it for you. In the desktop app, choose **Browse…** to pick the folder, or type the path.

## Add a project from GitHub

The **From GitHub** choice connects to GitHub and clones one of your repositories. It does not add a second project button; it is the GitHub source in the same Add project form.

1. Choose **Add project**.
2. Choose **From GitHub**.
3. Choose **Connect GitHub**.
4. Open the GitHub page and enter the code the app shows. Then approve the sign-in.
5. Type in the repository box to filter by owner or repository name. The list shows the clone address, whether the repository is private or public, and **Added** when a project already uses that repository.
6. Choose a repository. The name fills in from the repository and you can change it.
7. Choose the clone folder. The app clones the repository into a folder named after the repository inside that folder. Choose **Browse…** to pick another folder.
8. Choose **Clone and add project**.

The app clones with the GitHub token and adds the new folder as a project. It never overwrites an existing folder. If a clone fails, it removes the partial folder and adds no project. Git commands in the new project use the Git credentials and SSH keys on this computer, the same as a clone in a terminal.

The token stays in the system keychain. It is never written to the app's state file, a log or an API response. Choose **Disconnect** in the GitHub form to delete it.

## Project settings

Choose the settings button in the project header to set the defaults for new sessions in that project.

- **Default AI config** fills the config, provider and client in the launch form.
- **Default source branch** fills the source branch. The branch must exist in the project folder's Git repository.

Choose **Clear default AI config** or **Clear default branch** to remove a default. New sessions then start with no config or branch filled in.

The launch form uses the defaults when you choose the project. You can change the AI config or branch for one session without changing the project defaults.

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
