---
title: Require a login
description: Let only the people you choose reach a route, with Cloudflare Access.
---

A route is public by default: anyone with the URL reaches your app. To limit it to certain
people, edit the route, open **Advanced**, turn on **Require a login** and list who can
sign in:

- an email address, like `me@example.com`, lets in that person;
- `@example.com` lets in everyone with an address at that domain.

Visitors then see Cloudflare's sign-in page first. By default they get a one-time code by
email; any other login methods the account has (Google, GitHub, …) are offered too.

## What you need
Cloudflare **Zero Trust**, which is free for up to 50 users. If the account has never used
it, open Zero Trust in the Cloudflare dashboard once and choose a team name. Teitunnel says
so in the review if it isn't set up. Your API token also needs two account permissions:
**Access: Apps and Policies** (edit) and **Access: Organizations, Identity Providers, and
Groups** (edit, so Teitunnel can add One-time PIN when there's no login method yet).

## What Teitunnel changes
Like every change, you review the plan before anything happens:

- **Add a login method:** only when the account has none. Teitunnel adds One-time PIN.
- **Require a login:** an Access application named `Teitunnel · <hostname>` with one
  policy allowing the people you listed. It's created *before* the route goes live, so the
  route is never reachable without a login.
- Changing who can sign in updates that application. Turning the login off, or removing
  the route, deletes it, once the route is gone.

Teitunnel changes only applications it created. If a hostname is already protected by an
application made elsewhere, the plan stops and asks you to manage it in the dashboard.

A route with a path (for example `^/admin`) protects that path. Access matches plain paths,
so a path pattern like `^/(a|b)` can't have a login; use one route per path instead.

## Checking a protected route
**Test** reports "Works · asks for a login" when Cloudflare answers with its sign-in page.
That shows the hostname is on Cloudflare and protected; the app behind the login isn't
reached, because the check can't sign in.

## From the terminal

```sh
teitunnel-cli route add admin.example.com 3000 --allow me@example.com --allow @example.com
```
