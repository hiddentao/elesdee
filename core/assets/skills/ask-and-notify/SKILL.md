---
name: ask-and-notify
description: Ask the user through elesdee when you are blocked, wait for the answer, send short notifications, and report when the task is done.
---

# Ask and notify

## Ask when you are blocked

Call `ask_me` with one clear question. Add up to eight short `options` when the user must choose.

- Ask about a decision you cannot make safely: an ambiguous requirement, a destructive action, missing access or a choice between valid designs.
- Do not ask for information you can find in the repository, the task or the session output.
- After `ask_me`, continue with work that does not depend on the answer.
- Call `wait_for_answer` with the returned question id when you need the answer before you continue. `timeoutSeconds` is 1 to 25. A timeout returns "Still pending." Call it again while you remain blocked.
- Call `withdraw_question` when the question is no longer needed. Do not leave stale questions.

The question stays visible in the app until the user answers it, you withdraw it, or the session closes.

## Notify

Call `notify_me` for a short message that needs the user's attention now, such as a login that a build needs, a long job that finished, or a decision that unblocks other work.

- Keep the message under 300 characters.
- Do not notify for each step, each file change or routine progress. The user already sees the terminal.

## Report done

Call `report_done` when the task is complete and verified.

- The session stays active until the user closes it.
- Before you report done, leave the branch in a clean state: commit the work, run the checks you can run, and state what you did not verify.
