# teitunnel-cli in a container: run this machine's tunnels (`up`, the default), and
# manage routes with the same image (`docker run … teitunnel-cli route add …`).
#
# The Cloudflare API token comes from CLOUDFLARE_API_TOKEN or, better, a secret file
# (CLOUDFLARE_API_TOKEN_FILE); it's never written to the volume. State (which tunnels
# this container runs, the plan engine's records) lives in /data: keep it on a volume.
#
# Build: docker build -f docker/Dockerfile -t teitunnel .

ARG RUST_VERSION=1.98
ARG CLOUDFLARED_VERSION=2026.9.1

FROM rust:${RUST_VERSION}-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p teitunnel-cli \
    && cp target/release/teitunnel-cli /teitunnel-cli \
    && mkdir -p /data

# Cloudflare's own image, signed and published with each release.
FROM cloudflare/cloudflared:${CLOUDFLARED_VERSION} AS cloudflared

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /teitunnel-cli /usr/local/bin/teitunnel-cli
COPY --from=cloudflared /usr/local/bin/cloudflared /usr/local/bin/cloudflared
# Owned by `nonroot` (65532), so a fresh volume mounted here is writable.
COPY --from=build --chown=65532:65532 /data /data
ENV TEITUNNEL_DATA_DIR=/data \
    TEITUNNEL_CLOUDFLARED=/usr/local/bin/cloudflared
VOLUME ["/data"]
USER nonroot
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s \
    CMD ["teitunnel-cli", "routes", "--check"]
ENTRYPOINT ["teitunnel-cli"]
CMD ["up"]
