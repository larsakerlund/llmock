"""SDK-compatibility / e2e test for the OpenAI Responses API.

Drives llmock with the genuine OpenAI SDK's `client.responses` interface — if
the real SDK parses our `response` object and `response.*` stream events into
the expected values, our wire format is faithful to spec.

Run llmock first, then:
    LLMOCK_BASE_URL=http://127.0.0.1:8084/v1 \
        tests/sdk_compat/.venv/bin/python tests/sdk_compat/test_openai_responses.py
"""

import json
import os
import sys

from openai import OpenAI

BASE_URL = os.environ.get("LLMOCK_BASE_URL", "http://127.0.0.1:8080/v1")
client = OpenAI(base_url=BASE_URL, api_key="sk-llmock-dummy")

ok = True


def check(name, cond):
    global ok
    status = "PASS" if cond else "FAIL"
    print(f"[{status}] {name}")
    ok &= bool(cond)


# 1. Non-streaming text
r = client.responses.create(model="gpt-4o", input="hello there")
check("response object id has resp_ prefix", r.id.startswith("resp_"))
check("status completed", r.status == "completed")
check("output_text convenience works", r.output_text == "This is a default mock response from llmock.")
check("output[0] is a message", r.output[0].type == "message")
check("usage input/output tokens present", r.usage.input_tokens > 0 and r.usage.output_tokens > 0)
check("usage total tokens", r.usage.total_tokens == r.usage.input_tokens + r.usage.output_tokens)

# 2. Streaming text — reassemble from response.output_text.delta + final event
stream = client.responses.create(model="gpt-4o", input="hello there", stream=True)
deltas = ""
types = []
final_text = None
for ev in stream:
    types.append(ev.type)
    if ev.type == "response.output_text.delta":
        deltas += ev.delta
    elif ev.type == "response.completed":
        final_text = ev.response.output_text
check("stream begins with response.created", types[0] == "response.created")
check("stream ends with response.completed", types[-1] == "response.completed")
check("stream has output_item.added", "response.output_item.added" in types)
check("stream text reassembles", deltas == "This is a default mock response from llmock.")
check("completed event carries full output_text", final_text == "This is a default mock response from llmock.")

# 3. Non-streaming tool/function call
rt = client.responses.create(model="gpt-4o", input="give me the forecast for tokyo")
fc = next((item for item in rt.output if item.type == "function_call"), None)
check("output contains a function_call item", fc is not None)
check("function_call name", fc.name == "get_weather")
check("function_call call_id has call_ prefix", fc.call_id.startswith("call_"))
check(
    "function_call arguments parse to expected JSON",
    json.loads(fc.arguments) == {"location": "Tokyo", "unit": "celsius"},
)

# 4. Streaming tool/function call — reassemble arguments from delta events
stream_t = client.responses.create(model="gpt-4o", input="give me the forecast for tokyo", stream=True)
args = ""
saw_added_fc = False
saw_args_done = False
for ev in stream_t:
    if ev.type == "response.output_item.added" and ev.item.type == "function_call":
        saw_added_fc = True
        check("streamed function_call name on added item", ev.item.name == "get_weather")
    elif ev.type == "response.function_call_arguments.delta":
        args += ev.delta
    elif ev.type == "response.function_call_arguments.done":
        saw_args_done = True
        check("args.done carries full arguments", json.loads(ev.arguments) == {"location": "Tokyo", "unit": "celsius"})
check("stream emitted function_call added item", saw_added_fc)
check("stream emitted function_call_arguments.done", saw_args_done)
check("streamed tool arguments reassemble to JSON", json.loads(args) == {"location": "Tokyo", "unit": "celsius"})

print()
if ok:
    print("All Responses API SDK-compat checks passed.")
    sys.exit(0)
print("Some Responses API SDK-compat checks FAILED.")
sys.exit(1)
