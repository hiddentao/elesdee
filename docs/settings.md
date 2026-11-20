---
title: Settings
description: Manage accounts and launch presets, and read the app version.
order: 8
---

# Settings

Open **Settings** from the bottom of the sidebar. Settings opens over the session view. Close it to return to your work.

## Providers

The Providers page lists the accounts on this computer. Each row shows the account name, client, plan and usage summary. Sample accounts carry a **Sample** tag.

Choose **Add account** to connect a provider. Choose the provider, the sign-in method, and the fields it needs: an API key, a server URL, a model or an API style. See [Agents and providers](agents.md) for the provider list.

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
