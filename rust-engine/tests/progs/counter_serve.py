import json, sys

# One serve process for its instance: the count is the program's own, so an
# engine that plants a fresh instance per call would restate it (2.6 says one
# shared inner network per instance; 0014's finding 3 keeps the hosted one
# alive). `answer` says the running count, `pong` keeps a long-lived engine's
# ping from calling the process hung (3.2), and `cancel` frees the slot at
# once (3.2).
count = 0
print(json.dumps({"v": 1, "type": "ready"}), flush=True)
for line in sys.stdin:
    try:
        m = json.loads(line)
    except Exception:
        continue
    t = m.get("type")
    if t == "signal":
        count += 1
        print(json.dumps({"type": "answer", "re": m["re"], "payload": {"count": count}}), flush=True)
    elif t == "cancel":
        print(json.dumps({"type": "quiet", "re": m["re"]}), flush=True)
    elif t == "ping":
        print(json.dumps({"type": "pong", "n": m["n"]}), flush=True)
