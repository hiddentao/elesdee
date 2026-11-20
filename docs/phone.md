---
title: Phone layout
description: See and answer sessions in a phone-sized view.
order: 10
---

# Phone layout

The app has a phone-sized layout at `/phone.html`. It uses the same core service and the same sessions as the desktop window. It is a preview of the phone experience. The core service can hold the phone link identity and pair a phone that speaks the link protocol. A paired phone app can show the live session list, open a session and follow its output. The installable phone app is not released yet.

## Language

The globe button in the header opens **Languages**. Pick one of the 12 interface languages; the sheet marks the current one. The layout follows the language of the phone on first launch and keeps your choice. The phone language can differ from the desktop window.

## Session list

The list shows every session with its project, status and recent output. **Waiting for you** sits above the other sessions. The filters show all sessions, running sessions or sessions that need you. A paired phone app updates the list from the computer's event stream.

Tap a session to open it.

## Tabs

In the browser layout, the tab bar shows one session at a time. Scroll it sideways to move to the next session. The number button opens a sheet with every open session, grouped by **Waiting for you** and **Everything else**. The open session is marked.

Swipe a row left to reveal **Close**. No matter how far you swipe, it stops at the button and does not close the tab. Tap **Close** to hide the tab. The session keeps running and appears again in the session list.

## Session view

- The terminal is read-only on the phone.
- The output follows the session as it runs. If the connection drops, the app reconnects and continues from the last output it saw.
- A session created from the phone starts in its project folder. The phone cannot choose another folder.
- Approval requests, agent questions and session requests appear as cards. Answer or approve them there.
- The back button returns to the session list.

## Demo servers on the phone

A demo server is a loopback port that an agent registers for its session. The session's Files panel on the computer lists it with the local address and a Stop action.

A paired phone can ask for a port from the active list and forward it over the link to `127.0.0.1` on the computer. Only a registered port is allowed. There is one tunnel per port and at most eight at once. The tunnel closes when the server stops, the session closes or pauses, or the phone disconnects.

The installable app is not released. The desktop side of this path is built and tested. The phone shell's listener is not built, so no device test has run.

## What is not built yet

There is no phone app you can install. The repository has the phone shell, its pairing screens, the live session list and the session output view, but no released build. The downloaded phone interface and the phone shell's demo-forward listener come later. The core service starts the link, creates a pairing ticket, stores paired devices and answers the allowlisted methods for a paired phone, including the demo-server list and the computer side of demo forwarding. The current browser layout still opens on this computer, next to the desktop app.

## Next steps

- [Sessions](sessions.md)
- [Permissions](permissions.md)
