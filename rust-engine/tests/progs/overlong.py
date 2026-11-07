import sys, time
# Level 0: writes past 1 MiB on stdout and stays living on it (the over-long
# cut reports from the read; the engine kills the group at once).
sys.stdout.write("x" * 1048600)
sys.stdout.flush()
time.sleep(60)
