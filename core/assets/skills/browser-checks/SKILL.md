---
name: browser-checks
description: Check a UI change in this session's built-in browser, take screenshots and share them with the user.
---

# Browser checks

This session has its own browser. Its tabs, cookies, storage and logins belong to this session only. Do not expect the user's browser or another session's browser.

## When to use it

Use the browser tools when the work shows in a web page:

- a page the session's demo server serves on localhost;
- a local file served by a small static server;
- a public page the user asked you to check.

For a page on this computer, start a server in the folder and open its `http://localhost` or `http://127.0.0.1` address. Browsers treat localhost as a secure context.

## Check a page

1. Call `browser_open` with the full address. It returns the visible text.
2. Use `browser_read` to read the page after a change.
3. Use `browser_wait` with a selector or text when the page loads content later. It returns at the timeout if the content never appears.
4. Use `browser_click` and `browser_type` to work through a form or a flow. Select elements with CSS selectors.
5. Call `browser_screenshot` to share the result. The image appears in the session's view for the user, on every device.

## Report

- Say what you opened and what you saw.
- Share a screenshot for a visual change, not only a description.
- If the browser is unavailable, say so and give the address to the user. Do not describe a page you did not open.
- A closed session's browser cannot be used. The session must stay active.
