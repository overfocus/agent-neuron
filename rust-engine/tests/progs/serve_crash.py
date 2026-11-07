import sys, time
# Level 1 that crashes right after ready (exit 3), with a word on stderr.
sys.stdin.readline()
sys.stdout.write('{"v": 1, "type": "ready"}\n'); sys.stdout.flush()
time.sleep(0.05)
sys.stderr.write("the model went away\n")
sys.exit(3)
