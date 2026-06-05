"""SDK-compatibility / e2e test for the Google Gemini API.

Drives llmock with the genuine `google-genai` SDK — a third vendor, with a
URL-encoded action (`:generateContent`), camelCase wire fields, and a Google
error envelope. If the real SDK parses our bytes, the protocol is faithful.

Run llmock first, then:
    LLMOCK_GEMINI_BASE_URL=http://127.0.0.1:8086/gemini \
        e2e/sdk_compat/.venv/bin/python e2e/sdk_compat/test_gemini.py
"""

import os
import sys

from google import genai
from google.genai import types
from google.genai.errors import APIError

BASE_URL = os.environ.get("LLMOCK_GEMINI_BASE_URL", "http://127.0.0.1:8080/gemini")
client = genai.Client(api_key="llmock-dummy", http_options=types.HttpOptions(base_url=BASE_URL))
MODEL = "gemini-2.0-flash"

ok = True


def check(name, cond):
    global ok
    print(f"[{'PASS' if cond else 'FAIL'}] {name}")
    ok &= bool(cond)


# 1. Non-streaming text
r = client.models.generate_content(model=MODEL, contents="hello there")
check("text convenience works", r.text == "This is a default mock response from llmock.")
check("candidate role is model", r.candidates[0].content.role == "model")
check("finish_reason STOP", str(r.candidates[0].finish_reason) == "FinishReason.STOP")
check("usage total tokens", r.usage_metadata.total_token_count > 0)

# 2. Streaming text — reassemble chunk.text
streamed = ""
for chunk in client.models.generate_content_stream(model=MODEL, contents="hello there"):
    if chunk.text:
        streamed += chunk.text
check("stream text reassembles", streamed == "This is a default mock response from llmock.")

# 3. Non-streaming function call
rt = client.models.generate_content(model=MODEL, contents="give me the forecast for tokyo")
calls = rt.function_calls
check("function_calls present", calls is not None and len(calls) == 1)
check("function call name", calls[0].name == "get_weather")
check("function call args parsed", calls[0].args == {"location": "Tokyo", "unit": "celsius"})

# 4. Streaming function call
fc = None
for chunk in client.models.generate_content_stream(model=MODEL, contents="give me the forecast for tokyo"):
    if chunk.function_calls:
        fc = chunk.function_calls[0]
check("streamed function call name", fc is not None and fc.name == "get_weather")
check("streamed function call args", fc is not None and fc.args == {"location": "Tokyo", "unit": "celsius"})

# 5. Error injection — Google envelope → SDK APIError with the right code
try:
    client.models.generate_content(model=MODEL, contents="boom")
    check("429 raises APIError", False)
except APIError as e:
    check("429 raises APIError with code 429", e.code == 429)

print()
if ok:
    print("All Gemini SDK-compat checks passed.")
    sys.exit(0)
print("Some Gemini SDK-compat checks FAILED.")
sys.exit(1)
