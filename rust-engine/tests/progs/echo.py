import json, sys
# Level 0 for the maxInflight test: what it read, echoed (a queued call waits
# for its slot, 3.2).
print(json.dumps({"seen": json.loads(sys.stdin.read() or "null")}))
