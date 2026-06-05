# syntax=docker/dockerfile:1

# Build the release binary against the pinned MSRV. The full (non-slim) rust
# image carries the C toolchain ring's build needs; rustls uses ring, so no
# cmake/openssl is required. BuildKit cache mounts keep the registry and target
# warm across builds, so only changed crates recompile.
FROM rust:1.96-bookworm AS builder
WORKDIR /app
COPY . .
# TARGETARCH scopes the target cache per architecture, so a multi-arch build
# (amd64 + arm64 in one step) does not share /app/target between platforms and
# bake one arch's artifacts into the other's image. The registry cache holds
# arch-neutral crate sources, so it is shared.
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=target-${TARGETARCH} \
    cargo build --release --locked \
    && cp target/release/llmock /usr/local/bin/llmock

# Runtime on debian-slim: a shell and curl let the image ship a HEALTHCHECK on
# /healthz, and ca-certificates lets record mode reach real providers over TLS.
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
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
