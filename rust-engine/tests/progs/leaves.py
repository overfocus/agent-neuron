import json, subprocess, sys
# Level 0 that ends but leaves a child sleeping in its process group (the
# `leaves` case: the survivor is stopped at the end of the run, 5.4).
data = sys.stdin.read()
subprocess.Popen(["sleep", "60"])
sys.exit(0)
