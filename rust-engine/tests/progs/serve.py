import json, sys, threading, time
# Level 1 in Python: answers each signal's re after a delay in a thread; pings
# answered; a cancelled call replies quiet (any reply frees its slot, 3.2).
lock = threading.Lock()
def send(m):
    with lock:
        print(json.dumps(m), flush=True)
def call(m, delay, cancelled):
    # The delay is a real-time deadline (time.time), not a count of 10 ms
    # sleeps: a scheduler that stretches a 10 ms sleep must not turn a
    # `delay` of 1200 into seconds of wall time (the answer would arrive
    # after the neuron's timeout and be cancelled instead).
    deadline = time.time() + delay / 1000.0
    while not cancelled.is_set() and time.time() < deadline:
        time.sleep(0.01)
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
