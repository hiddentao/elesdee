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

The app uses a copy that is already on your computer. When you add an AI config and a client is missing, choose **Install** next to it. The app asks first, downloads the client from its vendor and keeps a private copy in its data folder. It checks the version before it uses the copy. You can also install the agent yourself and open the app again.

## Add a project

1. Choose **Add project**.
2. Enter a name.
3. Enter the folder for the project, or choose **Browse…** in the desktop app.
4. Save.

A project folder can be a Git repository or a plain folder. Agent sessions need a Git repository. Terminal sessions work in either.

To start from GitHub, choose **From GitHub** instead of **Local folder**. Choose **Connect GitHub**, open the GitHub page, and enter the code the app shows. Then search your repositories, choose one, set the clone folder, and choose **Clone and add project**. The app clones the repository and adds the folder as a project. It never overwrites an existing folder. The GitHub token stays in the system keychain, and **Disconnect** removes it.

## Add a provider

1. Open **Settings**.
2. Open **Providers**.
3. Choose **Add provider**.
4. Enter a name and a Google API key.
5. Save.

The key goes to the system keychain. It is not written to the app's state file, a log or a session output.

Sample providers and AI configs appear on a new install. They are marked **Sample** and have no usage data. They show how the lists work. Add your own provider to run real sessions.

## Start a session

1. Choose **New session**.
2. Choose a project.
3. Choose **Agent** or **Terminal**.
4. For an agent session, choose an AI config. The config adds its provider, client, model, effort and overrides to the launch.
5. Set the repository folder if you want a different one from the project folder. The list shows repositories used by earlier sessions.
6. For an agent session, optionally choose a source branch. The app fetches the remote first and starts from the latest commit, without changing your own checkout.
7. Optionally enter a session name.
8. Open **Additional settings** to enter the first prompt, add environment variables for this launch, and choose what to do with uncommitted changes. The first prompt names the session when the name is empty.
9. Choose **Start**.

The session opens in a tab. Its terminal is live. Type into it as you would in a normal terminal.

## Next steps

- [Projects](projects.md)
- [Sessions](sessions.md)
- [Agents and providers](agents.md)
