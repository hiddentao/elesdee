---
title: Phone layout
description: See and answer sessions in a phone-sized view.
order: 10
---

# Phone layout

The app has a phone-sized layout at `/phone.html`. It uses the same core service and the same sessions as the desktop window. It is a preview of the phone experience. The core service can hold the phone link identity and pair a phone that speaks the link protocol. A paired phone app can show the live session list, open a session, follow its messages and output, send terminal input and run the session actions. The installable phone app is not released yet.

## Paired phones

**Settings > Devices** lists the phones paired with this computer. Each row shows the phone name, its platform, when it last connected and whether it is connected now. **Remove** ends the phone's access at once and closes its live connection.

Choose **Pair a phone** in **Settings > Devices** on the computer. The computer shows a QR code. The phone app scans it and shows a six-digit code. Check that the same code appears on the computer, then approve the phone. The computer lists the phone when the pairing is complete. The phone shell has its own pairing screens too.

## Language

The globe button in the header opens **Languages**. Pick one of the 12 interface languages; the sheet marks the current one. The layout follows the language of the phone on first launch and keeps your choice. The phone language can differ from the desktop window.

## Session list

The list shows every session with its project, status and recent output. **Waiting for you** sits above the other sessions. The filters show all sessions, running sessions or sessions that need you. A paired phone app updates the list from the computer's event stream.

Tap a session to open it.

## Tabs

In the browser layout, the tab bar shows one session at a time. Scroll it sideways to move to the next session. The number button opens a sheet with every open session, grouped by **Waiting for you** and **Everything else**. The open session is marked.

Swipe a row left to reveal **Close**. No matter how far you swipe, it stops at the button and does not close the tab. Tap **Close** to hide the tab. The session keeps running and appears again in the session list.

## Session view

A session with a structured conversation opens on its messages. Your prompts and the agent's replies appear in full. A tool call, such as a command or a file read, is one line with the tool name and a short summary; tap it to open the detail and tap again to close it. A strip above the conversation shows the session state, the context that is still free and the branch, with a **Terminal** button.

Antigravity and terminal sessions have no structured source, so they open on the terminal. The button above the conversation is the one tap between the messages and the real read-only terminal for every other session.

- The terminal output is read-only. The terminal input sends text when you press Send; the message composer on the message view sends a whole prompt the same way.
- A key rail above the input sends Esc, Tab, Shift+Tab, Backspace, Delete, the four arrows, Ctrl+C and Ctrl+D. It also shows the keys of the session's agent, such as the permission-mode key of Claude Code, with a short purpose.
- Tap Shift, Control or Alt to latch it for the next key. The latch clears after one key. Hold an arrow, Backspace or Delete to repeat it. The keyboard button shows or hides the rail, and the rail scrolls sideways.
- The **+** on the rail opens **Keyboard keys**. It lists the agent's keys and your saved keys. Name a key such as `C`, `F5` or `PageUp`, add Shift, Control or Alt, and add it to the rail. You can save up to eight keys. They belong to the phone, not to one session.
- The conversation and the output follow the session as it runs. If the connection drops, the app reconnects and continues from the last item it saw.
- A session created from the phone starts in its project folder. The phone cannot choose another folder.
- **New session** on the session list asks for a project, an AI config, an optional source branch, an optional name and an optional first prompt.
- The session actions switch the AI config, open the usage panel and close the session. Close asks first, as it does on the computer.
- Approval requests, agent questions and session requests appear as cards. Answer or approve them there.
- The back button returns to the session list.

## Files, images, uploads and changes

**Files** and **Changes** sit beside the session actions.

**Files** lists the session folder one folder at a time. Tap a folder to open it, and **Up** returns to the parent. A text file opens in a read-only view. An image file, such as PNG, JPEG, GIF, WebP, AVIF or BMP, opens as a picture. A binary file, an image that is not supported and an image over the size cap each show a short explanation instead of a broken view.

**Upload a file** opens the file picker on the phone, or the browser's file picker in this layout. One file of up to 25 MiB is sent. It goes in elesdee's own folder for the session, not in the repository, with a private file mode. The name is reduced to one name, an existing file is never replaced, and a repeated name gets a number. The agent is told the path in its input.

**Changes** shows every file changed since the session branched off, committed or not, like a pull request view, with the lines added and removed. Tap a file to open its diff. A binary file has no text diff. A folder outside Git says so.

## Demo servers on the phone

A demo server is a loopback port that an agent registers for its session. The session's Files panel on the computer lists it with the local address and a Stop action. When Tailscale is installed and running, the panel also lists the server's tailnet address, which a device on the tailnet can open without the app.

A paired phone shell opens the session's active demo servers as a list. Each row shows the server's name, its address on the phone, the tailnet address when the computer recorded one, a state and an **Open** action. The phone listens on `127.0.0.1` at the same port when it is free, or at a free port, and forwards the connection over the link to `127.0.0.1` on the computer. Only a registered port is allowed. There is one tunnel per port and at most eight at once. A tunnel closes when its server stops, the session closes or pauses, the phone disconnects, the pairing is removed or the app exits.

The installable app is not released. The computer side and the phone shell listener are built and tested on the host. No device or emulator test has run, so the browser handoff and background behavior are unchecked. Android may pause the app after you switch to the browser; this build does not request a foreground service. The phone stops forwarding only when the platform stops the app.

## What is not built yet

There is no phone app you can install. The repository has the phone shell, its pairing screens, the live session list, the structured message view and output, terminal input and the key rail, the session actions, the file list with text and image previews, the diff view, the upload action, and the demo-server list with its forwarding listener, but no released build. The downloaded phone interface comes later. The core service starts the link, creates a pairing ticket, stores paired devices and answers the allowlisted methods for a paired phone, including terminal input, session start, close, AI config switch, the structured conversation stream, upload, the file and image reads, the change list and the demo-server list and both sides of demo forwarding. The phone shell's own screens show the same message view and file and change views as the browser layout, with the same one button to the read-only terminal, and they keep the composer and the key rail. The phone's system browser handoff, the native keychain, the camera, the file picker and behavior in the background need a device test, and the message view, the image preview and the upload have not run on a device. The current browser layout still opens on this computer, next to the desktop app.

## Next steps

- [Sessions](sessions.md)
- [Permissions](permissions.md)
