# syntax=docker/dockerfile:1

# Build the release binary against the pinned MSRV. The full (non-slim) rust
# image carries the C toolchain ring's build needs; rustls uses ring, so no
# cmake/openssl is required. BuildKit cache mounts keep the registry and target
# warm across builds, so only changed crates recompile. Base images are
# digest-pinned (Dependabot bumps them); the tag stays for readability.
FROM rust:1.96-bookworm@sha256:13c186980fa33cc12759b429662a1322939dbe697484b7c33b47dd2698d28460 AS builder
WORKDIR /app
COPY . .
# TARGETARCH scopes the target cache per architecture, so a multi-arch build
# (amd64 + arm64 in one step) does not share /app/target between platforms and
# bake one arch's binary into the other's image. The registry cache holds
# arch-neutral crate sources, so it is shared.
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=target-${TARGETARCH} \
    cargo build --release --locked \
    && cp target/release/llmock /usr/local/bin/llmock

# Runtime on debian-slim: curl powers the HEALTHCHECK and ca-certificates ships a
# system trust store for outbound TLS. apt versions are pinned for reproducible
# builds; bump them when a build fails on a 404 (Debian rotated the patch) by
# reading the new candidate from the base image: `apt-cache policy curl`.
FROM debian:bookworm-slim@sha256:0104b334637a5f19aa9c983a91b54c89887c0984081f2068983107a6f6c21eeb AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20230311+deb12u1 \
        curl=7.88.1-10+deb12u14 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --shell /usr/sbin/nologin llmock
COPY --from=builder /usr/local/bin/llmock /usr/local/bin/llmock
USER llmock

# Bind all interfaces so the server is reachable from outside the container; the
# bare binary defaults to 127.0.0.1, which would be unreachable when published.
ENV LLMOCK_HOST=0.0.0.0 \
    LLMOCK_PORT=8080
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=3s --retries=3 \
    CMD curl -fsS "http://localhost:${LLMOCK_PORT}/healthz" || exit 1

ENTRYPOINT ["llmock"]
