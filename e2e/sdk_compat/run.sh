#!/usr/bin/env bash
# End-to-end SDK-compatibility runner: build llmock, start it on an ephemeral
# port with the example fixtures, then drive it with the genuine OpenAI SDK
# across both the Chat Completions and Responses APIs. Exits non-zero if any
# check fails.
set -euo pipefail

cd "$(dirname "$0")/../.."

PORT="${PORT:-8088}"
VENV="e2e/sdk_compat/.venv"

if [ ! -x "$VENV/bin/python" ]; then
  echo "Creating venv and installing provider SDKs..."
  python3 -m venv "$VENV"
  "$VENV/bin/pip" -q install --upgrade pip \
    "openai==1.99.1" "anthropic==0.62.0" "google-genai==1.29.0"
fi

echo "Building llmock..."
cargo build -q

echo "Starting llmock on port $PORT..."
# Instant streaming here — these suites check wire-format correctness, not the
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
echo
echo "== OpenAI Chat Completions API =="
"$VENV/bin/python" e2e/sdk_compat/test_openai.py
echo
echo "== OpenAI Responses API =="
"$VENV/bin/python" e2e/sdk_compat/test_openai_responses.py
echo
echo "== Anthropic Messages API =="
"$VENV/bin/python" e2e/sdk_compat/test_anthropic.py
echo
echo "== Google Gemini API =="
"$VENV/bin/python" e2e/sdk_compat/test_gemini.py

echo
echo "All SDK-compat suites passed."
