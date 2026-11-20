---
title: Agents and providers
description: The coding agents elesdee runs and how accounts work.
order: 5
---

# Agents and providers

elesdee runs four coding agents:

| Client | Provider |
| --- | --- |
| Claude Code | Anthropic, and Anthropic-compatible providers |
| Codex | OpenAI, and OpenAI-compatible providers |
| OpenCode 2 | OpenRouter, and custom providers |
| Antigravity | Google Gemini |

The app uses the copy of each agent that is already installed on your computer. It does not bundle the agents.

## Agent check

The welcome screen and the app check for all four agents and show their versions. If an agent is missing, install it and open the app again. A session with a missing agent does not start.

## Accounts

An account connects a client to a provider. The Settings page lists the accounts on this computer.

**Sample accounts**

A new install includes sample accounts. They are marked **Sample**. They have no usage data and exist to show how the account list works. Add your own account for real provider setup.

**Google accounts**

The current build can add a Google account for Antigravity.

1. Open **Settings**.
2. Open **Providers**.
3. Choose **Add account**.
4. Enter a name.
5. Enter a Google API key.
6. Optionally change the model. The default is `gemini-3.8-flash-medium`.
7. Save.

The API key goes to the system keychain. The app writes only account metadata to its state file. It never writes the key to a log, an API response or session output.

When an Antigravity session starts, the app creates a private home folder for that session, writes a settings file that selects Gemini, and passes the key in the child process environment. It removes the private home when the session closes.

## How a session uses an account

The session starts the client executable. For Claude Code, Codex and OpenCode 2, the client uses its own sign-in and configuration. For Google, the app supplies the key as described above.

An agent session also needs a Git repository. The app creates a branch and worktree for it.

## Antigravity permission prompts

Antigravity keeps its permission prompts in its own terminal. The app does not show a permission card for Antigravity. Answer its prompts in the terminal.

The other clients also run their normal terminal interface. Claude Code additionally sends permission requests to the app, so those requests appear as cards. See [Permissions](permissions.md).

## Usage

Account usage appears where the provider reports a number. A Google account has no official usage source, so it shows **No usage data**. Usage belongs to the computer and is not part of a session.

## Next steps

- [Sessions](sessions.md)
- [Permissions](permissions.md)
- [Settings](settings.md)
