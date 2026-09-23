---
title: Several tunnels
description: Keep groups of routes apart on one machine, each with its own connector.
---

Every route normally goes on your machine's tunnel, which Teitunnel creates with the first
route. You can give the machine more tunnels, for example to keep **staging** routes apart
from **production**: each tunnel runs its own connector, so you can stop, restart or keep
one running on its own.

## Create a tunnel
Open **Tunnels**, choose **New tunnel** (+) and give it a name. Names are unique in the
Cloudflare account and appear in its dashboard. As with every change, you review the plan
first; nothing starts running until the tunnel has a route.

## Put routes on it
With more than one tunnel, **New Route** has a **Tunnel** menu. It starts on the machine's
default tunnel. The routes list then says which tunnel serves each route, and a route is
only live when its own tunnel's connector is.

A hostname is routed once per machine: Teitunnel refuses a route that another of your
tunnels already serves, so the DNS record can only point one way.

## Run and remove
In **Tunnels**, each of your machine's tunnels has **Start/Stop Here**, **Keep running
when Teitunnel quits** (Always-on), its own traffic and logs, and **Delete**. Deleting a
tunnel removes its routes and the DNS records Teitunnel created for them; your other
tunnels aren't touched. Private networks stay on the default tunnel.

The Doctor checks each tunnel on its own, and its fixes apply to the tunnel the problem is
on.

## Run an existing tunnel here
A tunnel made elsewhere in the account (in the dashboard, or by Teitunnel on another
machine) can run on this machine too: select it in **Tunnels** and choose **Run on This
Mac…**. Nothing changes in Cloudflare; its routes stay as they are and this machine serves
them as well. If another machine runs it at the same time, Cloudflare splits requests
between both, so only do this when this machine runs the same services (to serve one
hostname from several machines on purpose, use [load balancing](/guides/load-balancing/)).
Tunnels configured with a local config file can't be run this way; import their routes
instead.

## From the terminal

```sh
teitunnel-cli tunnels                                # this machine's tunnels
teitunnel-cli tunnel create staging
teitunnel-cli route add beta.example.com 4000 --tunnel staging
teitunnel-cli route remove beta.example.com          # finds the tunnel by itself
teitunnel-cli export config-yaml --tunnel staging
teitunnel-cli tunnel adopt home-lab                  # run an existing tunnel here too
teitunnel-cli tunnel delete staging
```
