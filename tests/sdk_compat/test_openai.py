"""SDK-compatibility / e2e test: drive llmock with the genuine OpenAI SDK.

This is the fidelity guarantee — if the real SDK parses our bytes into the
expected objects, our wire format is faithful. Run llmock first, e.g.:

    ./target/debug/llmock --port 8080 --fixtures fixtures/example.yaml

then:

    tests/sdk_compat/.venv/bin/python tests/sdk_compat/test_openai.py
"""

import os
import sys

from openai import OpenAI

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

print()
if ok:
    print("All SDK-compat checks passed.")
    sys.exit(0)
else:
    print("Some SDK-compat checks FAILED.")
    sys.exit(1)
