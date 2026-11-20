---
title: Introduction
description: What elesdee is and how it fits your work.
order: 1
---

# Introduction

elesdee is a desktop app for coding agents. It runs Claude Code, Codex, OpenCode 2 and Antigravity in a window on your computer. Each session gets a real terminal, a Git branch and a worktree, so agent work stays separate from your main checkout.

The app keeps your sessions, projects and accounts on this computer. It does not send usage or crash data anywhere.

## What you can do now

- Add projects and start agent or terminal sessions in them.
- Start an agent session from a chosen branch, with an optional first prompt.
- Give each agent session its own branch and worktree.
- Watch and type into the agent's real terminal.
- Approve or deny Claude Code permission requests from a card.
- Close, reopen, rename and delete sessions.
- Confirm before quitting while sessions run.
- Add provider accounts with a subscription, API key or local server, including custom compatible endpoints.
- Switch a session to another account, and let usage limits switch or pause it.

This is an early build. The pages here describe what the app does today. Features that are not built yet do not appear in these pages.

## Requirements

- macOS on Apple Silicon for the published build. Other platforms are not packaged yet.
- A coding agent installed on the same computer. elesdee detects Claude Code, Codex, OpenCode 2 and Antigravity.
- Git for agent sessions. Agent sessions need a Git repository. Terminal sessions also work in a plain folder.

## How the app is put together

The app has two parts. A local core service runs the terminals, manages Git branches and worktrees, and saves state. The desktop window shows the sessions and sends your input to the core service.

The core service listens only on this computer. It is not reachable from your network.

## Next steps

- [Get started](getting-started.md)
- [Learn about projects](projects.md)
- [Learn about sessions](sessions.md)
