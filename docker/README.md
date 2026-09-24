# Teitunnel

**Cloudflare Tunnel routes on servers and in containers.** The image has `teitunnel`
and Cloudflare's own `cloudflared`: add routes through reviewed plans (tunnel, DNS record
and optional login in one step), and run this machine's tunnels as the container's main
process.

[Website](https://teitunnel.teispace.com) · [Docs](https://teitunnel.teispace.com/docs/guides/servers/) · [Source](https://github.com/teispace/teitunnel) · MIT

## Run it

```sh
docker volume create teitunnel
docker run --rm -e CLOUDFLARE_API_TOKEN -v teitunnel:/data \
  teispace/teitunnel route add app.example.com http://web:80 --yes
docker run -d --name teitunnel --restart unless-stopped \
  -e CLOUDFLARE_API_TOKEN -v teitunnel:/data --network my-app teispace/teitunnel
```

The API token needs **Cloudflare Tunnel: Edit**, **DNS: Edit** and **Zone: Read**. It's
read from `CLOUDFLARE_API_TOKEN` or, better, a secret file (`CLOUDFLARE_API_TOKEN_FILE`),
and never written to the volume. Routes point at other containers by name when they share
a network. The health check is `teitunnel routes --check`.

With Docker Compose, see the
[Compose guide](https://teitunnel.teispace.com/docs/tutorials/docker-compose/).

## Tags and platforms

`latest`, a minor version (`0.1`) and an exact version (`0.1.1`), for `linux/amd64` and
`linux/arm64`. The same image is published as `ghcr.io/teispace/teitunnel`.

## Trust

- Built by GitHub Actions from each release's own `teitunnel`, after checking its
  checksums and build provenance; no rebuild.
- Runs as a non-root user on a distroless base; state lives only in `/data`.
- SBOM and SLSA provenance attached. Verify with the GitHub CLI:

  ```sh
  gh attestation verify oci://ghcr.io/teispace/teitunnel:latest --repo teispace/teitunnel
  ```

Made by [Teispace](https://teispace.com). Not affiliated with Cloudflare.
