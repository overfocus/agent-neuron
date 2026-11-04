# Level 0 neuron with a threshold, in Python (standard library only). The same program as hypothesis.mjs.
# stdin is {"payload": P, "inputs": [P, ...]}; argv[1]: how long the stand-in "model call" takes, in ms.
# Answers once, naming the detectors that agreed.
import json
import sys
import time

message = json.load(sys.stdin)
inputs = message["inputs"]
time.sleep(int(sys.argv[1]) / 1000)
first = inputs[0]
print(json.dumps({
    "alert": first["alert"],
    "service": first["service"],
    "detectors": sorted(i["detector"] for i in inputs),
}))
