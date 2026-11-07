import json, sys, threading, time
# Level 1 in Python: answers each signal's re after a delay in a thread; pings
# answered; a cancelled call replies quiet (any reply frees its slot, 3.2).
lock = threading.Lock()
def send(m):
    with lock:
        print(json.dumps(m), flush=True)
def call(m, delay, cancelled):
    waited = 0
    while waited < delay and not cancelled.is_set():
        time.sleep(0.01)
        waited += 10
    if cancelled.is_set():
        send({"type": "quiet", "re": m["re"]})
        return
    payload = m["payload"]
    n = payload.get("n", 0) if isinstance(payload, dict) else 0
    send({"type": "answer", "re": m["re"], "payload": {"n": n * 2}})
jobs = []
sys.stdin.readline()
send({"v": 1, "type": "ready", "maxInflight": 2})
delay = float(sys.argv[1]) if len(sys.argv) > 1 else 0
for line in sys.stdin:
    try:
        m = json.loads(line)
    except Exception:
        continue
    t = m.get("type")
    if t == "signal":
        cancelled = threading.Event()
        threading.Thread(target=call, args=(m, delay, cancelled), daemon=True).start()
        jobs.append((m["re"], cancelled))
    elif t == "cancel":
        for re_, ev in reversed(jobs):
            if re_ == m["re"]:
                ev.set()
    elif t == "ping":
        send({"type": "pong", "n": m["n"]})
