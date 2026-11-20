---
title: Settings
description: Set startup and sleep behavior, manage providers, AI configs and phones, and export diagnostics.
order: 8
---

# Settings

Open **Settings** from the bottom of the sidebar. Settings opens over the session view. Close it to return to your work.

## General

The General page holds two options for this computer. Both are off by default. A change takes effect at once and stays after a restart.

- **Start elesdee when the system starts** starts the core service in the background at login and resumes the saved sessions. The window opens only when you click the app. When another copy is already running, the system starts one copy.
- **Keep the computer awake while agents work** stops the computer from sleeping while an agent session runs. The phone stays connected during that time. The app releases the computer when no agent session runs. Terminal sessions, closed sessions and paused work do not hold it awake.

If the system does not accept a change, the page says so. The app does not claim the new state when the system refused it.

## Providers

A provider holds a credential and the address it connects to. One provider can serve several AI configs, so you enter a subscription or key once.

The Providers page lists the providers on this computer. Each row shows the provider name, the clients it can run, its plan and its usage summary. Sample providers carry a **Sample** tag. A subscription provider also shows **Connected** or **Not connected**.

The app reads usage from official sources. Claude Code reports its plan limits while a session runs, Codex reports them from its app server, and DeepSeek reports the provider balance. The usage panel shows the 5-hour and weekly limits or the balance, with the time of the last read. If a read fails, the last numbers stay and the provider says the reading is stale. Providers without an official source show **No usage data**.

Choose **Add provider** to connect one. Choose the vendor and the sign-in method, then enter the fields it needs: an API key, a server URL or an API style. A subscription sign-in starts the installed client's own login and adds the provider after the client confirms the credential. See [Agents and providers](agents.md) for the vendor list, the clients each vendor supports and the subscription steps.

Each provider row has **Edit** and **Remove**. Edit changes the name, sign-in method, server address or custom provider. The key field starts blank: leave it blank to keep the saved key, or type a new key to replace it. A subscription provider has **Sign in** or **Sign in again** in the same form. A provider that starts as a sample stops being a sample when you edit it.

The app refuses to remove a provider while an AI config uses it. Remove those configs first. Removing a provider deletes its saved key and usage.

## AI configs

An AI config is what a session runs and what you switch between. It names a provider and one client, and it can add a model, an effort level, extra command-line arguments, environment variables and agent config settings. Model, effort and overrides apply to that config only and are never written into your repositories or the agent's global files.

The AI configs page lists the configs on this computer. Each row shows the provider, client, model, effort and a summary of the overrides.

Choose **Add AI config**. Pick an existing provider, or choose **New provider** to create one without leaving the form. The form shows only the clients that provider supports. Then choose the client and the name; the default name is the provider name and the client in brackets, for example `DeepSeek (OpenCode 2)`. The model box loads the provider's model list in real time, and you can also type a model name that the provider does not return. The effort choices come from the client. Open **Additional settings** for the extra arguments, environment, agent config and the provider's server override.

Remove asks first. An active session that uses the config blocks removal until you switch it or close it. Closed sessions do not block removal.

Usage belongs to the provider. In a session, the provider pill shows the current config's provider usage; hover it for every limit and the session's context left, or click it to switch the session to another config. The chooser lists only the configs allowed for that session, shows their provider usage, marks the config the automatic switch would use next, and edits the allowed list. A new session allows the configs of its starting client by default.

Context left comes from Claude Code. Other clients show **Context left unknown**. Context compacts automatically.

## Languages

The Languages page has one choice: the interface language. The app offers English, German, French, Spanish, Italian, Brazilian Portuguese, Dutch, Polish, Swedish, Simplified Chinese, Japanese and Korean.

On first launch the app uses the language of this computer. If the app does not support that language, it uses English. The app keeps your choice. Dates and numbers follow the language too; there are no separate format settings.

Agent output, prompts, session logs and terminal content stay in the language they were written in. These pages are in English only.

## Devices

The Devices page lists the phones paired with this computer. Each row shows the phone's name, its platform, when it last connected and whether it is connected now. The list refreshes while the page is open.

**Remove** asks first. Removing a phone revokes it at once: its saved key stops working and its live connection closes. Pair the phone again to restore access.

With no paired phone, the page explains pairing. Pairing uses the code that the desktop app shows. The desktop pairing screen is not part of this build yet; the phone shell and the core pairing routes exist.

## Diagnostics

The core service writes a local log to `logs/core.log` in its data folder, by default `~/.elesdee`. The file is private to your user account and keeps the most recent events in one backup file.

**Export diagnostics** asks for a folder, then writes one JSON file there with the app version, this computer's system details, record counts, the General settings and the recent log lines. Attach that file to a GitHub issue. The bundle and the log remove provider keys, pairing secrets, pairing tickets, agent credentials and session output. They do not contain the state file, prompts, terminal content or agent conversations.

## About

The About page shows the app name and version, and the update state.

elesdee checks this repository's releases at launch and every few hours. When a newer version exists, it downloads the package in the background and checks its signature. The page then says that the version is ready and offers **Install and restart**. The app installs only the package that passed the signature check.

If a session is running, **Install and restart** asks first. Choose **Later** to install the update when every session is closed. The update saves the sessions, stops the core service, installs, starts again and resumes the sessions. An agent that was mid-task resumes its conversation after the restart.

The page also shows when the check failed, when the download failed, when the signature check failed, and when this build has no update signing key. In that last case, in-app updates are unavailable, and you install a newer release by hand.

## Theme

The window follows the system light or dark setting. The terminal follows it too.

## Next steps

- [Agents and providers](agents.md)
- [Troubleshooting](troubleshooting.md)
