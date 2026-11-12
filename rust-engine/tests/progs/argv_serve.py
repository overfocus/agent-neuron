import json, sys
# Level 1: answers every signal with its own arguments; pings answered.
def send(m):
    print(json.dumps(m), flush=True)
sys.stdin.readline()
send({"v": 1, "type": "ready"})
for line in sys.stdin:
    try:
        m = json.loads(line)
    except Exception:
        continue
    if m.get("type") == "signal":
        send({"type": "answer", "re": m["re"], "payload": sys.argv[1:]})
    elif m.get("type") == "ping":
        send({"type": "pong", "n": m["n"]})
