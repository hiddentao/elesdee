---
title: Get started
description: Install elesdee, add a project and start your first session.
order: 2
---

# Get started

## Install

Download the app from the GitHub releases page. The current build is unsigned. On first launch, macOS may block it. Open System Settings, go to Privacy & Security, and choose Open Anyway for elesdee.

To run the app from source:

```sh
npm install
npm run desktop
```

The desktop shell starts the core service and opens the window. For separate development runs, use:

```sh
npm run core
npm run dev
```

The core service listens on `127.0.0.1:4319`. Vite serves the UI and forwards `/api` to the core service.

## First launch

A new install has no projects. The welcome screen checks for the four supported agents and shows which ones are installed, with their versions.

Install any missing agent before you start a session with it. The app uses the copy that is already on your computer.

## Add a project

1. Choose **Add project**.
2. Enter a name.
3. Enter the folder for the project, or choose **Browse…** in the desktop app.
4. Save.

A project folder can be a Git repository or a plain folder. Agent sessions need a Git repository. Terminal sessions work in either.

## Add an account

1. Open **Settings**.
2. Open **Providers**.
3. Choose **Add account**.
4. Enter a name and a Google API key.
5. Save.

The key goes to the system keychain. It is not written to the app's state file, a log or a session output.

Sample accounts appear on a new install. They are marked **Sample** and have no usage data. They show how the account list works. Add your own account to run real sessions.

## Start a session

1. Choose **New session**.
2. Choose a project.
3. Choose **Agent** or **Terminal**.
4. For an agent session, choose a client and a preset. Each connected account is a preset; a stored preset adds its model and overrides to the launch.
5. Set the folder if you want a different one from the project folder.
6. For an agent session, optionally choose a source branch.
7. Optionally enter a first prompt. The app uses it to name the session when the name is empty.
8. Optionally enter a session name.
9. Open **Additional settings** to add environment variables for this launch.
10. Choose **Start**.

The session opens in a tab. Its terminal is live. Type into it as you would in a normal terminal.

## Next steps

- [Projects](projects.md)
- [Sessions](sessions.md)
- [Agents and providers](agents.md)
