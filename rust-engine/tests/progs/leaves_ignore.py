import subprocess, sys
# Level 0 that ends but leaves a child that ignores SIGTERM in the group: the
# cancel gets a term note and, 2 s later, a kill note (6.3), and the run
# waits for the group (6.2 rule 6).
data = sys.stdin.read()
subprocess.Popen(["sh", "-c", "trap '' TERM; sleep 60"])
sys.exit(0)
