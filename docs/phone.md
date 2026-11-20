---
title: Phone layout
description: See and answer sessions in a phone-sized view.
order: 10
---

# Phone layout

The app has a phone-sized layout at `/phone.html`. It uses the same core service and the same sessions as the desktop window. It is a preview of the phone experience. The core service can hold the phone link identity and pair a phone that speaks the link protocol. The installable phone app is not released yet.

## Language

The globe button in the header opens **Languages**. Pick one of the 12 interface languages; the sheet marks the current one. The layout follows the language of the phone on first launch and keeps your choice. The phone language can differ from the desktop window.

## Session list

The list shows every session with its project, status and recent output. **Waiting for you** sits above the other sessions. The filters show all sessions, running sessions or sessions that need you.

Tap a session to open it.

## Tabs

The tab bar shows one session at a time. Scroll it sideways to move to the next session. The number button opens a sheet with every open session, grouped by **Waiting for you** and **Everything else**. The open session is marked.

Swipe a row left to reveal **Close**. No matter how far you swipe, it stops at the button and does not close the tab. Tap **Close** to hide the tab. The session keeps running and appears again in the session list.

## Session view

- The terminal is read-only on the phone.
- Approval requests, agent questions and session requests appear as cards. Answer or approve them there.
- The back button returns to the session list.

## What is not built yet

There is no phone app you can install. The repository has the phone shell and its pairing screens, but no released build, and the remote screens come later. The core service starts the link, creates a pairing ticket and stores paired devices; only the session snapshot method is answered for a paired phone. The current layout still opens on this computer, next to the desktop app.

## Next steps

- [Sessions](sessions.md)
- [Permissions](permissions.md)
