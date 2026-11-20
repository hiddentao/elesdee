---
name: handoff
description: Write the standard handoff note, commit the work, and push the session branch before a switch to another client, a usage stop, or a close.
---

# Handoff

A handoff note tells the next agent what happened and what to do next. Write it when:

- the session switches to a different client (Claude Code, Codex, OpenCode 2 or Antigravity);
- the account is out of usage and the app asks you to stop;
- the user is about to close the session.

A switch between accounts of the same client needs no note, because the conversation continues.

## The note

Use these headings and keep each one short:

```text
Goal: one sentence.
Done: what is complete and verified.
In progress: what is partial, with the files or commands.
Next: the next concrete steps.
Decisions: choices the next agent must not undo.
Checks: commands run and their results.
Branch: the session branch name.
```

Add file paths only where they help the next agent find the work.

## Save the note

Call the `write_handoff_note` tool with the note text. The app keeps the note and gives it, with the end of the log, to the next agent that works on the session. Save the note before you commit, so a recovery after a crash still has it.

## Commit and push

1. Commit all work on the session branch. Do not leave uncommitted changes.
2. Use the note as the message of the last commit. Start the subject with `Handoff:` and put the note in the body.
3. Push the session branch to its upstream remote. If no upstream is set, push to the only remote.

Do not add the note as a file in the repository. The commit message carries it with the branch, and the next agent can read it with `git log -1`. At Close, if work is still uncommitted, the app commits it and puts the saved note in that commit message.

If a check failed or a step was skipped, say so in the note. Do not describe unfinished work as done.
