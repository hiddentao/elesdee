---
title: Agents and providers
description: The coding agents elesdee runs and how providers and AI configs work.
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

## Providers

A provider holds a credential and the address it connects to. An AI config connects a provider to one client and adds the model, effort and launch settings. The Settings page lists both.

**Sample providers and configs**

A new install includes sample providers and configs. They are marked **Sample**. They have no usage data and exist to show how the lists work. Add your own provider for real setup. Auto-switch skips sample providers.

**Supported vendors**

| Provider | Client | Sign-in |
| --- | --- | --- |
| Anthropic | Claude Code | Subscription or API key |
| OpenAI | Codex | Subscription or API key |
| Google | Antigravity | API key |
| DeepSeek | Claude Code | API key |
| Kimi (Moonshot) | Claude Code | API key |
| Z.ai | Claude Code | API key |
| OpenRouter | OpenCode 2 | API key |
| Ollama | Claude Code | Local server |
| LM Studio | Claude Code | Local server |
| Custom provider | Codex, Claude Code or OpenCode 2 | API key or local server |

**API keys**

1. Open **Settings**.
2. Open **Providers**.
3. Choose **Add provider**.
4. Choose the vendor.
5. Enter a name and the API key.
6. Save.

The key goes to the system keychain. The app writes only provider metadata to its state file. It removes the key from session output before the output is stored or shown.

**Subscriptions**

Choose **Subscription** as the sign-in. Sign in with the client itself on this computer first, for example with `claude auth login` or the Codex login command. The app uses that sign-in and does not ask for a provider password.

**Local servers**

Choose **Local server** and enter the server URL for Ollama or LM Studio. The app passes the URL to the client.

**Custom providers**

Choose **Custom provider** for any server that speaks the OpenAI or Anthropic API, such as a gateway or a company proxy.

1. Choose the API style: OpenAI-compatible or Anthropic-compatible.
2. Choose the client: Codex, Claude Code or OpenCode 2.
3. Enter the server URL, an optional API key and an optional model.
4. Save.

The client follows the style. OpenCode 2 is the default client.

## How a session uses an AI config

The session starts the client from its AI config and gives it the provider's environment. For Claude Code and Codex subscriptions, the client uses its own sign-in. For API-key providers, the app passes the key in the child process environment and redacts it from session output. For Google, the app creates a private home folder for the session, writes a settings file that selects Gemini, and removes the private home when the session closes.

## Skills

Claude Code, Codex and OpenCode 2 sessions get the same skill pack:

| Skill | What it covers |
| --- | --- |
| elesdee | The session branch and worktree, the tools, and the other skills |
| Demo servers | Starting a server, registering its port and giving you an address that opens on your phone |
| Ask and notify | Asking you when blocked, waiting for the answer, notifications and reporting done |
| Coordinate sessions | Helper sessions, briefs and merging their branches |
| Handoff | The handoff note, the last commit and the push |
| Frontend design | Visual direction, template defaults and redundant controls |

The skills live in a private folder for each session and are removed when the session closes. Antigravity keeps its own prompts and gets no elesdee skill pack.

## Permission prompts

Antigravity keeps its permission prompts in its own terminal. The app does not show a permission card for Antigravity. Answer its prompts in the terminal.

Claude Code, Codex and OpenCode 2 send permission requests to the app, so those requests appear as cards. See [Permissions](permissions.md).

## Usage

Provider usage appears where the provider reports a number. A Google provider has no official usage source, so it shows **No usage data**. Usage belongs to the computer and is not part of a session.

When a usage report shows 90% or more used, or a prepaid balance falls below its minimum, the app moves the session to an allowed AI config whose provider has room. Subscription providers come first, then pay-per-use providers; within each group it takes the provider with the most room, and a tie keeps the current client. It skips sample providers and providers without usage data. If no allowed config has room, the session pauses and keeps its worktree. It resumes by itself when an allowed config has room again. You can also choose **Resume** in the session.

Set the AI configs a session may use in the launch form, or edit them in the switch chooser while it runs. A new session allows the configs of its starting client. The config in use always stays allowed.

## Next steps

- [Sessions](sessions.md)
- [Permissions](permissions.md)
- [Settings](settings.md)
