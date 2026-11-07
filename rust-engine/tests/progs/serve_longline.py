import sys
# Level 1 that answers one call with a line past 1 MiB (an error output for
# exactly that call, and the process is killed, 3.2).
sys.stdin.readline()
sys.stdout.write('{"v": 1, "type": "ready"}\n'); sys.stdout.flush()
for line in sys.stdin:
    sys.stdout.write("z" * 1048600)
    sys.stdout.write("\n"); sys.stdout.flush()
