# syntax=docker/dockerfile:1

# Full (non-slim) rust image: ring (rustls's backend) needs a C toolchain, so no
# openssl/cmake. Digest-pinned so Dependabot bumps it deliberately.
FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS builder
WORKDIR /app
COPY . .
# Scope the target cache per arch, else a multi-arch build shares /app/target
# across platforms and bakes one arch's binary into the other's image.
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=target-${TARGETARCH} \
    cargo build --release --locked \
    && cp target/release/llmock /usr/local/bin/llmock

FROM debian:13.7-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a AS runtime
# The pinned base digest lags Debian's security updates, so upgrade to pick them
# up. A stale build-cache layer would mask new updates by replaying the old apt
# run, so CI feeds a changing value here to invalidate the layer and re-fetch.
ARG APT_CACHEBUST=0
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20250419 \
        curl=8.14.1-2+deb13u5 \
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
