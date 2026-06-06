# syntax=docker/dockerfile:1

# Full (non-slim) rust image: ring (rustls's backend) needs a C toolchain, so no
# openssl/cmake. Digest-pinned so Dependabot bumps it deliberately.
FROM rust:1.96-bookworm@sha256:13c186980fa33cc12759b429662a1322939dbe697484b7c33b47dd2698d28460 AS builder
WORKDIR /app
COPY . .
# Scope the target cache per arch, else a multi-arch build shares /app/target
# across platforms and bakes one arch's binary into the other's image.
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=target-${TARGETARCH} \
    cargo build --release --locked \
    && cp target/release/llmock /usr/local/bin/llmock

FROM debian:12.14-slim@sha256:0104b334637a5f19aa9c983a91b54c89887c0984081f2068983107a6f6c21eeb AS runtime
# The pinned base digest lags Debian's security updates, so upgrade to pick them up.
RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends \
        ca-certificates=20230311+deb12u1 \
        curl=7.88.1-10+deb12u14 \
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
