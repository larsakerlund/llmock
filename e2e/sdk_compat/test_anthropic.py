"""SDK-compatibility / e2e test for the Anthropic Messages API.

Drives llmock with the genuine `anthropic` SDK — a second *vendor*, proving the
neutral core serializes a fundamentally different wire format (named events,
content_block lifecycle, split usage, distinct error envelope) faithfully.

Run llmock first, then:
    LLMOCK_ANTHROPIC_BASE_URL=http://127.0.0.1:8085/anthropic \
        e2e/sdk_compat/.venv/bin/python e2e/sdk_compat/test_anthropic.py
"""

import json
import os
import sys

from anthropic import Anthropic, AuthenticationError, RateLimitError

BASE_URL = os.environ.get("LLMOCK_ANTHROPIC_BASE_URL", "http://127.0.0.1:8080/anthropic")
client = Anthropic(base_url=BASE_URL, api_key="sk-ant-llmock-dummy")
MODEL = "claude-opus-4-8"

ok = True


def check(name, cond):
    global ok
    print(f"[{'PASS' if cond else 'FAIL'}] {name}")
    ok &= bool(cond)


# 1. Non-streaming text
m = client.messages.create(
    model=MODEL, max_tokens=1024,
    messages=[{"role": "user", "content": "hello there"}],
)
check("message id has msg_ prefix", m.id.startswith("msg_"))
check("type message / role assistant", m.type == "message" and m.role == "assistant")
check("content[0] is text", m.content[0].type == "text")
check("text content", m.content[0].text == "This is a default mock response from llmock.")
check("stop_reason end_turn", m.stop_reason == "end_turn")
check("usage tokens", m.usage.input_tokens > 0 and m.usage.output_tokens > 0)

# 2. Streaming text — SDK stream helper reassembles
with client.messages.stream(
    model=MODEL, max_tokens=1024,
    messages=[{"role": "user", "content": "hello there"}],
) as stream:
    streamed = "".join(text for text in stream.text_stream)
    final = stream.get_final_message()
check("stream text_stream reassembles", streamed == "This is a default mock response from llmock.")
check("stream final message text", final.content[0].text == "This is a default mock response from llmock.")
check("stream final stop_reason", final.stop_reason == "end_turn")
check("stream final usage output_tokens", final.usage.output_tokens > 0)

# 3. Non-streaming tool use
mt = client.messages.create(
    model=MODEL, max_tokens=1024,
    messages=[{"role": "user", "content": "give me the forecast for tokyo"}],
)
tu = next((b for b in mt.content if b.type == "tool_use"), None)
check("tool_use block present", tu is not None)
check("tool_use stop_reason", mt.stop_reason == "tool_use")
check("tool_use id has toolu_ prefix", tu.id.startswith("toolu_"))
check("tool_use name", tu.name == "get_weather")
check("tool_use input is parsed object", tu.input == {"location": "Tokyo", "unit": "celsius"})

# 4. Streaming tool use — SDK accumulates input_json_delta into input dict
with client.messages.stream(
    model=MODEL, max_tokens=1024,
    messages=[{"role": "user", "content": "give me the forecast for tokyo"}],
) as stream:
    final_t = stream.get_final_message()
tus = next((b for b in final_t.content if b.type == "tool_use"), None)
check("streamed tool_use present", tus is not None)
check("streamed tool_use name", tus.name == "get_weather")
check("streamed tool_use input reassembled", tus.input == {"location": "Tokyo", "unit": "celsius"})

# 5. Error injection — Anthropic-shaped envelope → typed SDK exceptions
try:
    client.messages.create(model=MODEL, max_tokens=1024,
                           messages=[{"role": "user", "content": "boom"}])
    check("429 raises RateLimitError", False)
except RateLimitError as e:
    check("429 raises RateLimitError", e.status_code == 429)

try:
    client.messages.create(model=MODEL, max_tokens=1024,
                           messages=[{"role": "user", "content": "unauthorized please"}])
    check("401 raises AuthenticationError", False)
except AuthenticationError as e:
    check("401 raises AuthenticationError", e.status_code == 401)

print()
if ok:
    print("All Anthropic SDK-compat checks passed.")
    sys.exit(0)
print("Some Anthropic SDK-compat checks FAILED.")
sys.exit(1)
