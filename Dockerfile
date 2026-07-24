# syntax=docker/dockerfile:1

# Full (non-slim) rust image: ring (rustls's backend) needs a C toolchain, so no
# openssl/cmake. Digest-pinned so Dependabot bumps it deliberately.
FROM rust:1.96-bookworm@sha256:a339861ae23e9abb272cea45dfafde21760d2ce6577a70f8a926153677902663 AS builder
WORKDIR /app
COPY . .
# Scope the target cache per arch, else a multi-arch build shares /app/target
# across platforms and bakes one arch's binary into the other's image.
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=target-${TARGETARCH} \
    cargo build --release --locked \
    && cp target/release/llmock /usr/local/bin/llmock

FROM debian:13.6-slim@sha256:020c0d20b9880058cbe785a9db107156c3c75c2ac944a6aa7ab59f2add76a7bd AS runtime
# The pinned base digest lags Debian's security updates, so upgrade to pick them
# up. A stale build-cache layer would mask new updates by replaying the old apt
# run, so CI feeds a changing value here to invalidate the layer and re-fetch.
ARG APT_CACHEBUST=0
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20250419 \
        curl=8.14.1-2+deb13u3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --no-create-home --shell /usr/sbin/nologin llmock
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
