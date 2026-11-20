---
title: Troubleshooting
description: Fix common problems with agents, sessions and the app.
order: 8
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

The current build is unsigned. Open System Settings, go to Privacy & Security, and choose Open Anyway for elesdee. Signing and notarization will follow.

## An API key was rejected

Open Settings, remove the account and add it again with a valid key. The key is stored in the system keychain. The app does not show the saved key again.

## Where the app keeps its data

The core service stores its state under your home folder in `.elesdee`. The folder is private to your user account. Session logs and the state file live there.

## Next steps

- [Get started](getting-started.md)
- [Permissions](permissions.md)
