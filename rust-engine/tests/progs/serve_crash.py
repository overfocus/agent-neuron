import json, os, sys, time
# Level 1 that crashes right after ready (exit 3), with a word on stderr —
# on its FIRST start only: a restarted instance is a working serve that
# echoes like echo.py, so the scenario "the process restarts and the next
# call answers" (3.2) can actually happen. A marker file in the neuron's
# working directory (the declaring file's directory, 2.4) marks the crashed
# start; two instances never race here, because the engine restarts a level 1
# neuron only after the crashed one is gone.
sys.stdin.readline()
if not os.path.exists(os.path.join(os.getcwd(), "crashed.flag")):
    open(os.path.join(os.getcwd(), "crashed.flag"), "w").close()
    sys.stdout.write('{"v": 1, "type": "ready"}\n')
    sys.stdout.flush()
    time.sleep(0.05)
    sys.stderr.write("the model went away\n")
    sys.exit(3)
sys.stdout.write('{"v": 1, "type": "ready", "maxInflight": 2}\n')
sys.stdout.flush()
for line in sys.stdin:
    try:
        m = json.loads(line)
    except Exception:
        continue
    t = m.get("type")
    if t == "signal":
        sys.stdout.write(json.dumps({"type": "answer", "re": m["re"], "payload": {"seen": m["payload"]}}) + "\n")
        sys.stdout.flush()
    elif t == "cancel":
        sys.stdout.write(json.dumps({"type": "quiet", "re": m["re"]}) + "\n")
        sys.stdout.flush()
    elif t == "ping":
        sys.stdout.write(json.dumps({"type": "pong", "n": m["n"]}) + "\n")
        sys.stdout.flush()
