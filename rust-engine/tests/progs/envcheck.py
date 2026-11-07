import json, os, sys
# Level 0: reports which environment it saw (the isolation test's probe).
print(json.dumps({"path": bool(os.environ.get("PATH")), "extra": os.environ.get("EXTRA"), "secret": os.environ.get("SECRET")}))
