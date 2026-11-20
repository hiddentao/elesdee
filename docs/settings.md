---
title: Settings
description: Manage accounts and launch presets, and read the app version.
order: 8
---

# Settings

Open **Settings** from the bottom of the sidebar. Settings opens over the session view. Close it to return to your work.

## Providers

The Providers page lists the accounts on this computer. Each row shows the account name, client, plan and usage summary. Sample accounts carry a **Sample** tag.

The app reads usage from official sources. Claude Code reports its plan limits while a session runs, Codex reports them from its app server, and DeepSeek reports the account balance. The usage panel shows the 5-hour and weekly limits or the balance, with the time of the last read. If a read fails, the last numbers stay and the account says the reading is stale. Providers without an official source show **No usage data**.

Choose **Add account** to connect a provider. Choose the provider, the sign-in method, and the fields it needs: an API key, a server URL, a model or an API style. See [Agents and providers](agents.md) for the provider list.

Each account row has **Edit** and **Remove**. Edit changes the name, model, sign-in method, server address or custom provider. The key field starts blank: leave it blank to keep the saved key, or type a new key to replace it. An account that starts as a sample stops being a sample when you edit it.

Remove asks first. It deletes the saved key and usage, and clears presets and project defaults that use the account. An active session that uses the account blocks removal until you switch it or close it. Closed sessions do not block removal, but they cannot reopen on a removed account.

Usage belongs to this computer. The account rows show the remaining usage of each account. In a session, the provider pill shows the current account's usage; hover it for every limit and the session's context left, or click it to switch the session to another account. The chooser lists only the accounts allowed for that session, shows their remaining usage, marks the account the automatic switch would use next, and edits the allowed list. A new session allows the accounts of its starting client by default.

Context left comes from Claude Code. Other clients show **Context left unknown**. Context compacts automatically.

## Presets

A launch preset names an account and can add a model, environment variables and agent config settings. Every connected account appears here as a preset too, marked as coming from the account.

Choose **Add preset** to create one. Type a name, choose an account, and add the model and any overrides. Choose a row to edit it, or delete a stored preset you no longer need. Presets live in the app's own settings; nothing is written into your repositories. API keys stay in the system keychain and are never part of a preset.

## Languages

The Languages page has one choice: the interface language. The app offers English, German, French, Spanish, Italian, Brazilian Portuguese, Dutch, Polish, Swedish, Simplified Chinese, Japanese and Korean.

On first launch the app uses the language of this computer. If the app does not support that language, it uses English. The app keeps your choice. Dates and numbers follow the language too; there are no separate format settings.

Agent output, prompts, session logs and terminal content stay in the language they were written in. These pages are in English only.

## About

The About page shows the app name and version.

## Theme

The window follows the system light or dark setting. The terminal follows it too.

## Version

The version also appears at the bottom of the Settings navigation.

## Next steps

- [Agents and providers](agents.md)
- [Troubleshooting](troubleshooting.md)
