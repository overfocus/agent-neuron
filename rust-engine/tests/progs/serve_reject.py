import json, sys, time
# Level 1 that answers with a `to` naming a neuron it has no link to (3.2).
sys.stdin.readline()
sys.stdout.write('{"v": 1, "type": "ready"}\n'); sys.stdout.flush()
for line in sys.stdin:
    try: m = json.loads(line)
    except Exception: continue
    if m.get("type") == "signal":
        time.sleep(0.02)
        sys.stdout.write(json.dumps({"type": "answer", "re": m["re"], "payload": "x", "to": ["nowhere"]}) + "\n"); sys.stdout.flush()
