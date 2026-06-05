#!/usr/bin/env bash
# End-to-end SDK-compatibility runner: build llmock, start it on an ephemeral
# port with the example fixtures, then drive it with the genuine provider SDKs
# (OpenAI Chat + Responses, Anthropic Messages, Google Gemini). Exits non-zero
# if any check fails.
#
# Python dependencies are managed by uv from the pinned uv.lock in this
# directory; uv provisions an interpreter and the SDKs on first run and caches
# them afterwards. uv is the only prerequisite.
set -euo pipefail

cd "$(dirname "$0")/../.."

PORT="${PORT:-8088}"
PROJECT="e2e/sdk_compat"

echo "Syncing e2e dependencies (uv)..."
uv sync --locked --project "$PROJECT" -q

echo "Building llmock..."
cargo build -q

echo "Starting llmock on port $PORT..."
# Instant streaming here: these suites check wire-format correctness, not the
# realistic default timing (which is validated separately).
./target/debug/llmock --port "$PORT" --fixtures fixtures/example.yaml \
  --default-ttft-ms 0 --default-inter-token-ms 0 --default-jitter-ms 0 \
  >/tmp/llmock-e2e.log 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

# Wait for readiness, then fail fast if the server never comes up.
ready=0
for _ in $(seq 1 50); do
  if curl -fs "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.2
done
if [ "$ready" -ne 1 ]; then
  echo "Error: llmock did not become healthy on port $PORT. Server log:" >&2
  cat /tmp/llmock-e2e.log >&2 || true
  exit 1
fi

export LLMOCK_BASE_URL="http://127.0.0.1:$PORT/openai/v1"
export LLMOCK_ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT/anthropic"
export LLMOCK_GEMINI_BASE_URL="http://127.0.0.1:$PORT/gemini"

run() { uv run --locked --project "$PROJECT" python "$PROJECT/$1"; }

echo
echo "== OpenAI Chat Completions API =="
run test_openai.py
echo
echo "== OpenAI Responses API =="
run test_openai_responses.py
echo
echo "== Anthropic Messages API =="
run test_anthropic.py
echo
echo "== Google Gemini API =="
run test_gemini.py

echo
echo "All SDK-compat suites passed."
