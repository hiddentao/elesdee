# AGENTS.md

This repository holds the elesdee product spec, research notes and clickable UI mockups. There is no app code yet.

This file carries the rules that bind every prompt. Detail and decisions live in `.dev/`.

## Where things are

| Path | Contents |
| --- | --- |
| `.dev/SPEC.md` | The product spec. The source of truth for behavior. |
| `.dev/mockups/round-1.html` | The clickable Mac and phone mockups. A is dark, B is light. |
| `.dev/research/` | Research notes behind the spec. |
| `AGENTS.md` | These rules. |

## Keep this file and `.dev/` current

- **Every change updates the documents it affects, in the same commit.** That includes `.dev/SPEC.md` and this file.
- **Rules stay here. Detail goes in `.dev/`.** Link the detail; do not repeat it.
- **A fact in more than one place changes in every place in the same commit.** The mockup, the spec and these rules must agree.
- **A new topic starts as a `.dev/` document.** Add it to the map above.
- **No app code until the spec says so.** The mockups exist to choose a direction first.

## Hard rules

- **Sessions have two states: Active and Closed.** Working, waiting, paused and reported done are details inside Active. Only the user closes a session.
- **Do not add redundant interface elements or duplicate functionality.** Before adding a control, check whether the same action already exists in the same context. Keep one clear control instead of several shortcuts.
- **Open-ended dropdowns must be filterable combo boxes with asynchronous loading.** Never use a native `<select>` or a fixed list for values that can grow without a known bound: projects, labels, branches, folders, accounts, sessions, files, servers and similar. The user types to filter. Options load asynchronously with a visible loading state. Keep keyboard navigation, focus handling and screen-reader labels.
- **The mockup is self-contained.** No network calls, webfonts or external assets. It is a prototype, not app code.
- **Usage belongs to the Mac, not to a session.** The sidebar summary opens the global usage panel. A session's side panel holds Files, Changes and Browser only.
- **Each session has its own browser context.** Tabs, cookies, storage and logins never cross sessions.
- **The user closes a session.** An agent can report done, but cannot close.

## Default skills

- **frontend-design** — load it before any UI, mockup or visual design work in this repo.

## Verification

- Open `.dev/mockups/round-1.html` in Chrome at 1440 and 390 widths. Check both directions and both devices.
- There must be no console errors and no horizontal page scroll.
- Check the changed interaction in both directions before you commit.
