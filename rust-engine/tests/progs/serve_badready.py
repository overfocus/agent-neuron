import json, sys
# Level 1 whose ready line carries the wrong protocol version (3.2).
sys.stdin.readline()
sys.stdout.write(json.dumps({"v": 2, "type": "ready"}) + "\n"); sys.stdout.flush()
