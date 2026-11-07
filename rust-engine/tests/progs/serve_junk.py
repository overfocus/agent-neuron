import json, sys
# Level 1 that writes a junk line while one call is running, then answers
# (a junk line fails exactly that call, 3.2).
first = True
sys.stdin.readline()
sys.stdout.write('{"v": 1, "type": "ready"}\n'); sys.stdout.flush()
for line in sys.stdin:
    try: m = json.loads(line)
    except Exception: continue
    if m.get("type") == "signal":
        if first:
            first = False
            sys.stdout.write("this line is junk\n"); sys.stdout.flush()
        sys.stdout.write(json.dumps({"type": "answer", "re": m["re"], "payload": "ok"}) + "\n"); sys.stdout.flush()
