---
title: Troubleshooting
description: Fix common problems with agents, sessions and the app.
order: 9
---

# Troubleshooting

## An agent shows as not installed

The app runs the copy of the agent on your computer. Install the agent, then open the app again so the check runs.

## An agent session does not start

Agent sessions need a Git repository. Check that the folder is inside a Git repository, not a plain folder. For a plain folder, start a terminal session instead.

## The app cannot start

The core service listens on `127.0.0.1:4319`. If another copy of elesdee is already running, stop it first. The app starts one core service, and a second one cannot take the same port.

In development, check that the core service runs before the UI. Vite forwards `/api` requests to the core service.

## A Claude Code prompt stays in the terminal

Permission cards cover mutating tools only. Read-only tools, questions and plan approval stay in the terminal. Answer them there.

## Antigravity shows no permission card

This is expected. Antigravity keeps its prompts in its own terminal. Answer them there.

## The permission card does not appear

The card belongs to Claude Code sessions. Codex, OpenCode 2 and Antigravity keep their prompts in their terminals in the current build.

If a Claude Code card is missing, check that the session is still active. A closed session denies any request that was still pending.

## Closing a session kept the worktree

The app keeps the worktree when it cannot commit, push or remove it. The session preview says why. Check the repository and the remote, then close the session again or remove the worktree with Git.

## The app is blocked on first launch

A build without a Developer ID signature is blocked by macOS. The release notes say whether the build is signed. If it is unsigned, open System Settings, go to Privacy & Security, and choose Open Anyway for elesdee.

## An API key was rejected

Open Settings, remove the provider and add it again with a valid key. The key is stored in the system keychain. The app does not show the saved key again.

## The About page says updates are unavailable

This build has no update signing key. It cannot check for or install updates. Download the newest release from this repository's GitHub releases page and install it by hand.

## The update check failed

The app could not reach GitHub. Check the network connection, then choose **Check for updates** on the About page.

## The update did not pass its signature check

The app refused the package and did not install it. Check for updates again. If the same package fails again, install the release by hand from this repository's GitHub releases page.

## The update did not install

The app keeps running and stops the install before it replaces the app. Choose **Install and restart** again. The sessions stay saved.

## The built-in browser did not download

The app uses Chrome or Chromium when one is installed. Otherwise the first use of a session browser downloads a pinned Chrome for Testing build into the app's data folder. The Browser panel shows the step and the progress. If the download or its check fails, the panel shows the reason and a **Retry** button.

Check the network connection and the free space on the disk, then choose **Retry**. A failed download is removed, so the retry starts clean. Installing Chrome or Chromium also works: the next use takes the installed copy and needs no download.

## Report a bug with diagnostics

Open **Settings**, choose **Diagnostics**, and choose **Export diagnostics**. Pick a folder. The app writes one JSON file there, named `elesdee-diagnostics-<timestamp>.json`. Attach that file to a GitHub issue.

The bundle holds the app version, the operating system, the architecture, record counts, the General settings and the recent log lines. It does not hold provider keys, pairing secrets, pairing tickets, agent credentials, prompts, session output, terminal content or agent conversations.

The same log is at `.elesdee/logs/core.log` under your home folder. The app keeps one older file as `core.log.1` and redacts each line before it writes it. Do not attach the state file: it holds local state, not diagnostics.

## Where the app keeps its data

The core service stores its state under your home folder in `.elesdee`. The folder is private to your user account. Session logs and the state file live there. A downloaded browser build lives in the `chromium` folder there; delete that folder to make the next browser use download again.

## Next steps

- [Get started](getting-started.md)
- [Permissions](permissions.md)
