import importlib.util
import json
import sys

spec = importlib.util.spec_from_file_location("order_item", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = mod
spec.loader.exec_module(mod)

from pydantic import TypeAdapter

adapter = TypeAdapter(mod.OrderItem)
cases = [
    ({"id": "a", "quantity": 2, "tags": ["x", "y"]}, True),
    ({"id": "a", "quantity": 1}, True),
    ({"id": "a", "quantity": 0}, False),
    ({"id": "a", "quantity": 1, "tags": ["x", "x"]}, False),
    ({"id": "a", "quantity": 1, "unknown": True}, False),
    ({"quantity": 1}, False),
]
ok = True
for payload, expected in cases:
    try:
        adapter.validate_python(payload)
        actual = True
    except Exception:
        actual = False
    if actual != expected:
        ok = False
        print(f"MISMATCH: {json.dumps(payload)} expected={expected} actual={actual}")
print("PASS" if ok else "FAIL")
