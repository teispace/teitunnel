# Teitunnel for VS Code

Share your dev server at a public address without leaving the editor, through the
[Teitunnel](https://teitunnel.teispace.com) app on your computer (free, on your own
Cloudflare account). Works in VS Code, Cursor and Windsurf.

- **Share This Workspace's Dev Server**: finds the port from `package.json` scripts (Vite,
  Next.js, Astro, Angular, webpack, …) or the framework (Django, Rails, Laravel, Flask),
  checks it's running, shares it and copies the address.
- **Share Port…**, and **Share with Teitunnel** on any port in the Ports view.
- **Shares and Routes** view: copy an address, open it, stop a share, open its request
  inspector or show it in Teitunnel.
- **Status bar**: how many shares are live.
- **Run Doctor**: Teitunnel's checks, fixed in the app.
- **Request notifications** (off by default, `teitunnel.notifyRequests`).

## Requirements

The Teitunnel app, running, with **Settings ▸ Integrations ▸ Allow connections** on (the
default). The extension talks to it over a local socket (or named pipe on Windows) only
you can open, with a token from Teitunnel's data folder; nothing goes over the network.
Sharing or stopping asks in Teitunnel first, unless you choose **Always Allow** for
`vscode` there.

In remote windows (SSH, containers, WSL) the extension runs on your computer, where the
app is: share a forwarded port from the Ports view.
