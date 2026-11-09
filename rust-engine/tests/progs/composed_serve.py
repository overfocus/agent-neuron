import json, sys, threading, time

# One serve call per slot, recorded for the test that reads the log: `start`
# when the call opens, `cancel` when the engine's cancel line reaches it (the
# call replies quiet at once, 3.2), `answer` when the delay runs out. The
# arguments are maxInflight (the program's own word in ready, 3.2), the delay
# in ms, and the log path.
lock = threading.Lock()


def send(m):
    with lock:
        print(json.dumps(m), flush=True)


def rec(what, re_):
    with lock:
        with open(sys.argv[3], "a") as f:
            f.write(json.dumps({"what": what, "re": re_}, separators=(",", ":")) + "\n")


sys.stdin.readline()
send({"v": 1, "type": "ready", "maxInflight": int(sys.argv[1])})
delay = float(sys.argv[2]) / 1000.0
cancels = {}


def run(re_):
    cancelled = threading.Event()
    with lock:
        cancels[re_] = cancelled
    rec("start", re_)
    deadline = time.time() + delay
    while not cancelled.is_set() and time.time() < deadline:
        time.sleep(0.01)
    if cancelled.is_set():
        rec("cancel", re_)
        send({"type": "quiet", "re": re_})
        return
    rec("answer", re_)
    send({"type": "answer", "re": re_, "payload": {"slow": 1}})


for line in sys.stdin:
    try:
        m = json.loads(line)
    except Exception:
        continue
    t = m.get("type")
    if t == "signal":
        threading.Thread(target=run, args=(m["re"],), daemon=True).start()
    elif t == "cancel":
        ev = cancels.get(m["re"])
        if ev is not None:
            ev.set()
    elif t == "ping":
        send({"type": "pong", "n": m["n"]})
