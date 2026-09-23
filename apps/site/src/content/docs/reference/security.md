---
title: Security
description: How Teitunnel protects your credentials, your DNS and your Mac.
---

The full model is in [SECURITY_MODEL.md](https://github.com/teispace/teitunnel/blob/main/docs/SECURITY_MODEL.md).
To report a vulnerability, see [SECURITY.md](https://github.com/teispace/teitunnel/blob/main/SECURITY.md).

## Credentials

- Cloudflare tokens and tunnel run tokens are stored **only in the macOS keychain**,
  never in files, logs or the app's database.
- The window never sees them: its requests name an account, and the core fetches the
  token itself.
- A connector running while the app runs gets its run token in memory, never on the
  command line (where other users could see it). An [Always-on](/teitunnel/concepts/run-modes/)
  connector reads it from a file only you can read, removed when you turn Always-on off.

## Your DNS

- Every change is shown before it's made and recorded in Activity.
- Only records Teitunnel created are ever deleted without asking. See
  [DNS ownership](/teitunnel/concepts/dns-ownership/).

## Your Mac

- Nothing is exposed until you share or route it.
- Processes are started with explicit arguments, never through a shell.
- Metrics and diagnostics endpoints listen on `127.0.0.1` only.
- cloudflared downloads are checked against GitHub's digest, Cloudflare's published
  checksum and Cloudflare's code signature before use.
- Logs and diagnostics exports have tokens, keys and passwords removed.
