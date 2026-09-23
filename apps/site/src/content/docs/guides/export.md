---
title: Export your setup
description: Take this Mac's tunnel and routes to a server, to Docker, or into Terraform.
---

**Routes ▸ Export** turns this Mac's tunnel and routes into configuration for other tools.
Nothing secret is ever included.

| Format | What it's for |
|---|---|
| **config.yml** | The routes as a cloudflared configuration file: to run them as a locally-managed tunnel, or to keep as a record. |
| **Docker Compose** | A `cloudflared` service that runs this same tunnel in Docker, pinned to the version this Mac uses. The run token comes from `TUNNEL_TOKEN` in a `.env` file; get it with `cloudflared tunnel token <id>`. |
| **Terraform** | The tunnel, its routes and their DNS records for the Cloudflare provider v5, with `import` blocks, so `terraform plan` adopts what already exists instead of recreating it. |

Copy the file or save it to Downloads.

:::caution
After you import the Terraform, Terraform owns those resources: make changes there, not
in Teitunnel, or each will undo the other's edits.
:::

:::note
In Docker, `localhost` is the container itself. Point routes at
`host.docker.internal` or at other services' names so they're reachable from the container.
:::
