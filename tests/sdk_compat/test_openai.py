"""SDK-compatibility / e2e test: drive llmock with the genuine OpenAI SDK.

This is the fidelity guarantee — if the real SDK parses our bytes into the
expected objects, our wire format is faithful. Run llmock first, e.g.:

    ./target/debug/llmock --port 8080 --fixtures fixtures/example.yaml

then:

    tests/sdk_compat/.venv/bin/python tests/sdk_compat/test_openai.py
"""

import os
import sys

from openai import AuthenticationError, OpenAI, RateLimitError

BASE_URL = os.environ.get("LLMOCK_BASE_URL", "http://127.0.0.1:8080/v1")

client = OpenAI(base_url=BASE_URL, api_key="sk-llmock-dummy")


def check(name, cond):
    status = "PASS" if cond else "FAIL"
    print(f"[{status}] {name}")
    return cond


ok = True

# 1. Models listing
models = client.models.list()
ids = [m.id for m in models.data]
ok &= check("models.list returns gpt-4o", "gpt-4o" in ids)

# 2. Retrieve a single model
m = client.models.retrieve("gpt-4o")
ok &= check("models.retrieve echoes id", m.id == "gpt-4o" and m.object == "model")

# 3. Chat completion — fixture match on 'weather'
resp = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "what is the weather today?"}],
)
ok &= check("chat completion object type", resp.object == "chat.completion")
ok &= check("chat id has chatcmpl- prefix", resp.id.startswith("chatcmpl-"))
ok &= check(
    "weather fixture content",
    "sunny" in resp.choices[0].message.content,
)
ok &= check("role is assistant", resp.choices[0].message.role == "assistant")
ok &= check("finish_reason stop", resp.choices[0].finish_reason == "stop")
ok &= check("usage total_tokens present", resp.usage.total_tokens == 21)

# 4. Fallback fixture
resp2 = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "hello there"}],
)
ok &= check("fallback fixture content", "default mock response" in resp2.choices[0].message.content)

# 5. Streaming — the genuine SDK must parse our chunk stream
stream = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "the weather?"}],
    stream=True,
)
assembled = ""
roles = []
finish = None
all_chunk_type_ok = True
for chunk in stream:
    all_chunk_type_ok &= chunk.object == "chat.completion.chunk"
    choice = chunk.choices[0]
    if choice.delta.role:
        roles.append(choice.delta.role)
    if choice.delta.content:
        assembled += choice.delta.content
    if choice.finish_reason:
        finish = choice.finish_reason
ok &= check("every chunk is chat.completion.chunk", all_chunk_type_ok)
ok &= check("stream reassembles to full text", assembled == "It's sunny and 22°C with a light breeze.")
ok &= check("stream first delta carried role=assistant", roles == ["assistant"])
ok &= check("stream finish_reason stop", finish == "stop")

# 6. Streaming with usage on the final chunk
stream2 = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "the weather?"}],
    stream=True,
    stream_options={"include_usage": True},
)
usage = None
text2 = ""
for chunk in stream2:
    if chunk.usage is not None:
        usage = chunk.usage
    if chunk.choices and chunk.choices[0].delta.content:
        text2 += chunk.choices[0].delta.content
ok &= check("include_usage yields a usage object", usage is not None and usage.total_tokens == 21)
ok &= check("usage chunk has empty choices, text still complete", text2 == "It's sunny and 22°C with a light breeze.")

# 7. Error injection — the SDK must raise the right typed exception
try:
    client.chat.completions.create(
        model="gpt-4o",
        messages=[{"role": "user", "content": "boom"}],
    )
    ok &= check("429 raises RateLimitError", False)
except RateLimitError as e:
    ok &= check("429 raises RateLimitError", e.status_code == 429)
    ok &= check("error envelope code surfaced", e.body.get("code") == "rate_limit_exceeded")

try:
    client.chat.completions.create(
        model="gpt-4o",
        messages=[{"role": "user", "content": "unauthorized please"}],
    )
    ok &= check("401 raises AuthenticationError", False)
except AuthenticationError as e:
    ok &= check("401 raises AuthenticationError", e.status_code == 401)

# 8. Truncate fault — the stream ends without a normal completion. We only
#    require that some content arrived and the loop terminates (no hang).
chunks_seen = 0
try:
    for _chunk in client.chat.completions.create(
        model="gpt-4o",
        messages=[{"role": "user", "content": "please truncate this"}],
        stream=True,
    ):
        chunks_seen += 1
except Exception:
    # An abrupt mid-stream close may surface as a transport error; that's fine —
    # the point is the developer can simulate a dropped stream.
    pass
ok &= check("truncate fault streamed some chunks then ended", chunks_seen >= 1)

# 9. Tool/function calling — non-streaming
import json

resp_tool = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "give me the forecast for tokyo"}],
)
msg = resp_tool.choices[0].message
ok &= check("tool call: content is null", msg.content is None)
ok &= check("tool call: finish_reason tool_calls", resp_tool.choices[0].finish_reason == "tool_calls")
ok &= check("tool call: one tool_call present", msg.tool_calls is not None and len(msg.tool_calls) == 1)
tc = msg.tool_calls[0]
ok &= check("tool call: type function", tc.type == "function")
ok &= check("tool call: name", tc.function.name == "get_weather")
ok &= check("tool call: id has call_ prefix", tc.id.startswith("call_"))
ok &= check(
    "tool call: arguments parse to expected JSON",
    json.loads(tc.function.arguments) == {"location": "Tokyo", "unit": "celsius"},
)

# 10. Tool/function calling — streaming (reassemble argument fragments by index)
stream_tool = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "give me the forecast for tokyo"}],
    stream=True,
)
names = {}
args = {}
finish_t = None
for chunk in stream_tool:
    choice = chunk.choices[0]
    if choice.delta.tool_calls:
        for d in choice.delta.tool_calls:
            if d.function and d.function.name:
                names[d.index] = d.function.name
            if d.function and d.function.arguments:
                args[d.index] = args.get(d.index, "") + d.function.arguments
    if choice.finish_reason:
        finish_t = choice.finish_reason
ok &= check("stream tool call: name reassembled", names.get(0) == "get_weather")
ok &= check(
    "stream tool call: arguments reassembled to JSON",
    0 in args and json.loads(args[0]) == {"location": "Tokyo", "unit": "celsius"},
)
ok &= check("stream tool call: finish_reason tool_calls", finish_t == "tool_calls")

print()
if ok:
    print("All SDK-compat checks passed.")
    sys.exit(0)
else:
    print("Some SDK-compat checks FAILED.")
    sys.exit(1)
