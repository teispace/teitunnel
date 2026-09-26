# Security Policy

Teitunnel handles Cloudflare credentials and puts local services on the internet, so we take
every report seriously and are grateful to the people who make them.

## Supported versions

| Version | Supported |
|---|---|
| The latest release | Yes |
| Anything older | No: update to the latest release |

Before 1.0, fixes ship only in a new release. The app updates itself, and every channel
(Homebrew, apt, dnf, Docker) follows the latest release.

## Reporting a vulnerability

**Please don't report security issues in public issues, discussions or pull requests.**

Report it privately through GitHub's
[private vulnerability reporting](https://github.com/teispace/teitunnel/security/advisories/new).
If you can't use GitHub, email **info@teispace.com** with "Security" in the subject.

A helpful report includes:

- the affected version and operating system;
- what an attacker can do, and what they need first (for example, local access or a
  malicious web page);
- steps to reproduce, or a proof of concept;
- any idea you have for a fix.

Never include live credentials such as a Cloudflare API token. Redacted diagnostics
(**Export Diagnostics** in the app) are fine.

## What happens next

1. We acknowledge your report within **3 business days**.
2. We confirm and assess it within **10 business days**, and keep you updated as we work on
   a fix.
3. We agree on a disclosure date with you. We aim to release a fix within **90 days**, much
   sooner for severe issues.
4. We publish a GitHub security advisory, with a CVE where it applies, and credit you unless
   you'd rather stay anonymous.

## Scope

In scope:

- the desktop app, the `teitunnel` command, the Docker image and the release artifacts;
- the MCP server, the inspector (Lens), local HTTPS domains, the control connection and the
  browser and editor integrations;
- the Workers Teitunnel deploys to a user's account (Snapshots, offline page, webhook inbox);
- the release and update pipeline, and the website at `teitunnel.teispace.com`.

Out of scope:

- vulnerabilities in Cloudflare's own services or in `cloudflared`: report those to
  [Cloudflare](https://www.cloudflare.com/disclosure/);
- attacks that need an already-compromised computer or administrator access to it;
- volumetric denial of service, spam, and social engineering;
- reports from automated scanners without a demonstrated impact.

## Safe harbor

We won't pursue legal action against anyone who researches and reports a vulnerability in
good faith under this policy: who avoids privacy violations, data destruction and service
disruption, tests only against their own accounts and data, and gives us reasonable time to
fix the issue before disclosing it.

## How Teitunnel protects users

The design is documented in [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md): how
credentials, processes, DNS ownership, network exposure and the supply chain are protected.
