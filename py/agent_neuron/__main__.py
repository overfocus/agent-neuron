import sys

from .conform import conform


def main(argv):
    if len(argv) >= 2 and argv[0] == "conform":
        only = None
        rest = argv[1:]
        if "--only" in rest:
            i = rest.index("--only")
            only = rest[i + 1] if i + 1 < len(rest) else None
            rest = rest[:i] + rest[i + 2:]
        if len(rest) != 1 or ("--only" in argv and only is None):
            print("usage: python -m agent_neuron conform <dir> [--only <text>]", file=sys.stderr)
            return 2
        return conform(rest[0], only)
    print("usage: python -m agent_neuron conform <dir> [--only <text>]", file=sys.stderr)
    return 2


sys.exit(main(sys.argv[1:]))
