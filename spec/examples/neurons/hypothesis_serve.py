# Level 1 neuron with a threshold, in Python (standard library only): hypothesis.py as one long-lived process.
# argv[1]: how long the stand-in "model call" takes, in ms. Each call waits in its own thread, so a cancel or ping
# is read while a call runs; a cancelled call replies quiet, which frees its slot.
import json, sys, threading

lock, stops = threading.Lock(), {}                     # stops: re -> Event, set when the engine cancels that call

def send(m):
    with lock:
        print(json.dumps(m), flush=True)
def call(m, stop):
    cancelled = stop.wait(int(sys.argv[1]) / 1000)
    stops.pop(m["re"], None)
    if cancelled:
        return send({"type": "quiet", "re": m["re"]})  # any reply frees the slot
    inputs = m["inputs"]
    send({"type": "answer", "re": m["re"], "payload": {
        "alert": inputs[0]["alert"], "service": inputs[0]["service"], "detectors": sorted(i["detector"] for i in inputs)}})

json.loads(sys.stdin.readline())                       # hello
send({"v": 1, "type": "ready", "maxInflight": 4})
for line in sys.stdin:                                 # end of input = shut down
    m = json.loads(line)
    if m["type"] == "signal":
        stops[m["re"]] = threading.Event()
        threading.Thread(target=call, args=(m, stops[m["re"]]), daemon=True).start()
    elif m["type"] == "cancel" and m["re"] in stops:
        stops[m["re"]].set()
    elif m["type"] == "ping":
        send({"type": "pong", "n": m["n"]})
