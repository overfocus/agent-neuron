import sys, time
# Level 1 that never answers a ping (the pong deadline hangs the process).
sys.stdin.readline()
sys.stdout.write('{"v": 1, "type": "ready"}\n'); sys.stdout.flush()
for line in sys.stdin:
    time.sleep(0.01)
