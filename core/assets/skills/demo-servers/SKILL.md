---
name: demo-servers
description: Start a demo server for the work, register its loopback port, and give the user an address that opens on this computer, on the paired phone, and on the tailnet when Tailscale runs.
---

# Demo servers

A demo server shows the work in a browser: a page to review, a live preview, or the visual output of the code. Start the server, register its port, and give the user the address.

## Register the server

1. Start the server on a loopback address, such as `127.0.0.1` or `localhost`, on a fixed port. Choose a free port between 1024 and 65535.
2. Wait until the server answers before you register it.
3. Call `register_demo_server` with a short `name` and the `port`.
4. Print the addresses that the tool returns. The local address, such as `http://localhost:3000`, works in a browser on this computer. On the paired phone, the app forwards the same port. When the tool also returns a Tailscale address, print it for devices on the tailnet that do not have the app.

The tool refuses a privileged port, the elesdee service port, a port that another active session registered, a closed session, and a ninth server.

## Rules

- Bind loopback only. Do not bind all interfaces for a demo server.
- Use one port for one server. A session holds at most 8 servers.
- Stop, Close and pause stop the servers. Register the port again after the session resumes.
- The forwarded port gives the phone the same path, so links, live reload and cookies keep working.
- Do not serve secrets through a demo server. Treat every page as visible to the paired phone.
- Stop the server process yourself when the demo is no longer useful, then tell the user.
