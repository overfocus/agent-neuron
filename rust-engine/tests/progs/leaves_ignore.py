import subprocess, sys, time
# Level 0 that is cancelled while it is still running (the run's own timeout),
# leaves a child that ignores SIGTERM in the group, and dies on the SIGTERM
# itself: the cancel gets a term note and, 2 s later, a kill note (6.3), and
# the run waits for the group (6.2 rule 6). This is the fixture situation of
# `staleness/cut-leader-exits-on-term-a-survivor-is-killed` on real processes.
#
# The leader has to outlive the call's timeout: a program that ends by itself
# completes its call with that outcome (the trace reports it before the
# timeout), and the group's later stop at the end of the run happens silently
# (decision 0001), so the notes could never appear.
data = sys.stdin.read()
subprocess.Popen(["sh", "-c", "trap '' TERM; sleep 60"])
time.sleep(5)
