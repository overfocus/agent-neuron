import json, os, sys
# Level 0: reports its working directory's last name (the cwd test's probe).
print(json.dumps({"cwd": os.path.basename(os.getcwd())}))
