"""Network file loading and the errors of SPEC 2.7 that stop an engine from starting.

Standard library only. Written from SPEC section 2 and spec/network.schema.json's
description (closed schema, literal values), not from the TypeScript checker.
"""
import json
import math
import os
import re

ID_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$")
ENV_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
KINDS = ("source", "run", "serve", "http", "human", "network")
COMMON = {"id", "description", "at"}  # `at`: descriptive, for a drawing (decision 0011)
OPTS = {
    "source": set(),
    "run": {"threshold", "within", "maxHops", "timeout", "env", "cwd", "cost", "effect", "may", "maxInflight"},
    "serve": {"threshold", "within", "maxHops", "timeout", "env", "cwd", "cost", "effect", "may"},
    "http": {"threshold", "within", "maxHops", "timeout", "cost", "effect", "maxInflight"},
    "human": {"threshold", "within", "maxHops", "timeout", "cost", "effect"},
    "network": {"maxHops", "timeout", "cost", "may", "maxInflight"},
}
TOP = {"$schema", "version", "name", "description", "neurons", "links", "entry", "exit", "budget", "ground"}
GROUND = {"deposit", "evaporate", "select", "base", "power", "seed"}
MAX_AMOUNT = 2**53 - 1
MAY = ("connect", "listen", "read", "write")  # what a program may touch (SPEC 2.4, decision 0016)
HOST_RE = re.compile(r"^[a-z0-9]([a-z0-9.-]{0,251}[a-z0-9])?$")


class CheckError(Exception):
    pass


def _isint(v):
    return isinstance(v, int) and not isinstance(v, bool)


def _isnum(v):
    """A JSON number (4.5): not a bool, and one a double can hold, since the engine ages values in floats."""
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return False
    try:
        return math.isfinite(v)
    except OverflowError:
        return False


def _literal(s, what):
    if not isinstance(s, str) or "${" in s or "{{" in s:
        raise CheckError(f"{what}: not a literal string")


def _units(v, what):
    if not isinstance(v, dict) or not v:
        raise CheckError(f"{what}: must be a non-empty object of units")
    for k, a in v.items():
        if not ID_RE.match(k):
            raise CheckError(f"{what}: bad unit name {k!r}")
        if not _isint(a) or a < 0 or a > MAX_AMOUNT:
            raise CheckError(f"{what}: amount of {k} must be a whole number from 0 to 2^53-1")


def _host_port(s):
    """`host:port` (2.4): lower-case letters, digits, dots and hyphens, then a port from 1 to 65535."""
    host, sep, port = s.rpartition(":")
    if not sep or not HOST_RE.match(host):
        return False
    return port.isascii() and port.isdigit() and port[0] != "0" and int(port) <= 65535


def _may(m, what):
    """A `may` (2.4): a closed object of four members, each an array of unique literal strings."""
    if not isinstance(m, dict):
        raise CheckError(f"{what}: may must be an object")
    extra = set(m) - set(MAY)
    if extra:
        raise CheckError(f"{what}: may: unknown member {sorted(extra)[0]}")
    for member, items in m.items():
        if not isinstance(items, list) or not all(isinstance(x, str) for x in items) or len(set(items)) != len(items):
            raise CheckError(f"{what}: may.{member} must be an array of unique strings")
        for x in items:
            _literal(x, f"{what}.may.{member}")
            if member in ("connect", "listen"):
                if not _host_port(x):
                    raise CheckError(f"{what}: may.{member}: {x!r} is not host:port")
            elif not x:
                raise CheckError(f"{what}: may.{member}: a path is not empty")


def _ground(g, ids, path):
    """A `ground` of SPEC 4.5 (decision 0012 item 5): the closed shape, and the name `by` must declare."""
    if not isinstance(g, dict):
        raise CheckError(f"{path}: ground must be an object")
    extra = set(g) - GROUND
    if extra:
        raise CheckError(f"{path}: ground: unknown field {sorted(extra)[0]}")
    if "deposit" in g:
        dep = g["deposit"]
        if not isinstance(dep, dict):
            raise CheckError(f"{path}: deposit must be an object")
        extra = set(dep) - {"on", "add", "by"}
        if extra:
            raise CheckError(f"{path}: deposit: unknown field {sorted(extra)[0]}")
        for f in ("on", "add"):
            if f not in dep:
                raise CheckError(f"{path}: deposit needs a {f}")
            if not isinstance(dep[f], str) or not ID_RE.match(dep[f]):
                raise CheckError(f"{path}: deposit.{f} must be a name")
        if "by" in dep:
            if not isinstance(dep["by"], str) or not ID_RE.match(dep["by"]):
                raise CheckError(f"{path}: deposit.by must be a name")
            if dep["by"] not in ids:
                raise CheckError(f"{path}: deposit.by {dep['by']!r} names no declared neuron")
    if "evaporate" in g and (not _isnum(g["evaporate"]) or g["evaporate"] < 0):
        raise CheckError(f"{path}: evaporate must be a number from 0 up")
    if "select" in g and g["select"] is not True:
        raise CheckError(f"{path}: select must be true")
    if "base" in g and (not _isnum(g["base"]) or g["base"] < 0):
        raise CheckError(f"{path}: base must be a number from 0 up")
    if "power" in g and (not _isnum(g["power"]) or g["power"] <= 0):
        raise CheckError(f"{path}: power must be a number above 0")
    if "seed" in g and (not _isint(g["seed"]) or g["seed"] < 0):
        raise CheckError(f"{path}: seed must be a whole number from 0 up")


# A neuron named by its own file (SPEC 2.10, decision 0019). The entry that names it holds what is the
# network's, and may restate `timeout`, `env`, `may` and `maxInflight` only to narrow them; the file holds the rest.
ENTRY = COMMON | {"neuron", "threshold", "within", "maxHops", "cost", "timeout", "env", "may", "maxInflight"}
FILE_KINDS = ("run", "serve", "http", "human")


def _resolve(n, i, base):
    """The neuron an entry with `neuron` stands for, as it would be written inline; any other entry as it is."""
    if not isinstance(n, dict) or "neuron" not in n:
        return n
    what = f"neurons[{i}]"
    if any(k in n for k in KINDS):
        raise CheckError(f"{what}: exactly one kind field required")
    extra = set(n) - ENTRY
    if extra:
        raise CheckError(f"{what}: {sorted(extra)[0]} is the neuron's own: it is written in the neuron's file")
    _literal(n["neuron"], "neuron")
    path = os.path.join(base, n["neuron"])
    try:
        with open(path, encoding="utf-8") as fh:
            of = json.load(fh)
    except (OSError, ValueError) as e:
        raise CheckError(f"{what}: the neuron file {n['neuron']}: {e}")
    name = f"the neuron file {n['neuron']}"
    if not isinstance(of, dict):
        raise CheckError(f"{name}: not an object")
    kinds = [k for k in FILE_KINDS if k in of]
    if len(kinds) != 1:
        raise CheckError(f"{name}: exactly one of run, serve, http, human required")
    program = kinds[0] in ("run", "serve")
    own = {"$schema", "version", "description", kinds[0], "timeout", "effect"} | ({"env", "cwd", "may"} if program else set())
    limited = kinds[0] in ("run", "http")  # `maxInflight` (2.4): a serve program says its own, a person's queue is the desk's
    if limited:
        own.add("maxInflight")
    extra = set(of) - own
    if extra:
        raise CheckError(f"{name}: {sorted(extra)[0]} is not the neuron's own")
    if of.get("version") != 0 or isinstance(of.get("version"), bool):
        raise CheckError(f"{name}: version must be 0")
    # narrowing: the entry may ask for less than the file gives, never for more
    limit = of.get("timeout", 30000)
    if "timeout" in n and (not _isint(n["timeout"]) or not _isint(limit) or n["timeout"] > limit):
        raise CheckError(f"{what}: timeout is wider than the neuron's own ({limit})")
    for key in ("env", "may"):
        if key in n and not program:
            raise CheckError(f"{what}: {key} narrows a program's, and {name} is not a run or serve neuron")
    if "maxInflight" in n:
        if not limited:
            raise CheckError(f"{what}: maxInflight is not an option of a {kinds[0]} neuron, and {name} is one")
        # a file that states none has no limit, so any limit on the entry narrows it
        if "maxInflight" in of and (not _isint(n["maxInflight"]) or not _isint(of["maxInflight"]) or n["maxInflight"] > of["maxInflight"]):
            raise CheckError(f"{what}: maxInflight is wider than the neuron's own ({of['maxInflight']})")
    if "env" in n:
        theirs = of.get("env", [])
        if not isinstance(n["env"], list) or not isinstance(theirs, list) or any(x not in theirs for x in n["env"]):
            raise CheckError(f"{what}: env is wider than the neuron's own")
    if "may" in n:
        mine, theirs = n["may"], of.get("may", {})
        if not isinstance(mine, dict) or not isinstance(theirs, dict):
            raise CheckError(f"{what}: may is wider than the neuron's own")
        for member, items in mine.items():
            listed = theirs.get(member, [])
            if not isinstance(items, list) or not isinstance(listed, list) or any(x not in listed for x in items):
                raise CheckError(f"{what}: may.{member} is wider than the neuron's own")
    merged = {k: v for k, v in of.items() if k not in ("$schema", "version")}
    merged.update({k: v for k, v in n.items() if k != "neuron"})
    if program:
        if "cwd" in of:
            _literal(of["cwd"], "cwd")
        # the file's relative paths are relative to the file's own directory
        merged["cwd"] = os.path.abspath(os.path.join(os.path.dirname(path), of.get("cwd", ".")))
    return merged


def _neuron(n, i):
    if not isinstance(n, dict):
        raise CheckError(f"neurons[{i}]: not an object")
    kinds = [k for k in KINDS if k in n]
    if len(kinds) != 1:
        raise CheckError(f"neurons[{i}]: exactly one kind field required")
    kind = kinds[0]
    extra = set(n) - COMMON - {kind} - OPTS[kind]
    if extra:
        raise CheckError(f"neurons[{i}]: unknown field {sorted(extra)[0]}")
    if not isinstance(n.get("id"), str) or not ID_RE.match(n["id"]):
        raise CheckError(f"neurons[{i}]: bad id")
    if "description" in n and not isinstance(n["description"], str):
        raise CheckError(f"neurons[{i}]: description must be a string")
    if "at" in n:
        at = n["at"]
        ok = isinstance(at, list) and len(at) == 3 and all(
            isinstance(x, (int, float)) and not isinstance(x, bool) and abs(x) <= 1_000_000 and x == x for x in at
        )
        if not ok:
            raise CheckError(f"neurons[{i}]: at is three numbers from -1000000 to 1000000")
    v = n[kind]
    if kind in ("source", "human"):
        if v is not True:
            raise CheckError(f"neurons[{i}]: {kind} must be true")
    elif kind in ("run", "serve"):
        if not isinstance(v, list) or not v:
            raise CheckError(f"neurons[{i}]: {kind} must be a non-empty command array")
        for a in v:
            _literal(a, f"neurons[{i}].{kind}")
    elif kind == "http":
        _literal(v, "http")
        if not re.match(r"^https?://", v):
            raise CheckError(f"neurons[{i}]: http must be an http(s) URL")
    else:
        _literal(v, "network")
    if "within" in n and "threshold" not in n:
        raise CheckError(f"neurons[{i}]: within needs threshold")
    if "threshold" in n and (not _isint(n["threshold"]) or n["threshold"] < 1):
        raise CheckError(f"neurons[{i}]: threshold must be an integer >= 1")
    for f in ("within", "timeout"):
        top = 86400000 if f == "timeout" and "human" in n else 3600000  # a person may take a day (SPEC 2.4)
        if f in n and (not _isint(n[f]) or not 1 <= n[f] <= top):
            raise CheckError(f"neurons[{i}]: {f} must be an integer from 1 to {top}")
    if "maxHops" in n and (not _isint(n["maxHops"]) or n["maxHops"] < 0):
        raise CheckError(f"neurons[{i}]: maxHops must be an integer >= 0")
    if "maxInflight" in n and (not _isint(n["maxInflight"]) or n["maxInflight"] < 1):
        raise CheckError(f"neurons[{i}]: maxInflight must be an integer >= 1")
    if "env" in n:
        e = n["env"]
        if not isinstance(e, list) or len(set(e)) != len(e) or not all(isinstance(x, str) and ENV_RE.match(x) for x in e):
            raise CheckError(f"neurons[{i}]: bad env")
    if "cwd" in n:
        _literal(n["cwd"], "cwd")
    if "cost" in n:
        _units(n["cost"], f"neurons[{i}].cost")
    if "effect" in n and n["effect"] is not True:
        raise CheckError(f"neurons[{i}]: effect must be true")
    if "may" in n:
        _may(n["may"], f"neurons[{i}]")
    return kind


def load(path, _stack=()):
    """Return (data, dir) for a network file that passes check, or raise CheckError.

    Composed files are loaded recursively into data["_inner"][neuron id].
    """
    path = os.path.abspath(path)
    if path in _stack:
        raise CheckError(f"{path}: a network file contains itself")
    try:
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError) as e:
        raise CheckError(f"{path}: {e}")
    base = os.path.dirname(path)
    if not isinstance(data, dict):
        raise CheckError(f"{path}: not an object")
    extra = set(data) - TOP
    if extra:
        raise CheckError(f"{path}: unknown field {sorted(extra)[0]}")
    if data.get("version") != 0 or isinstance(data.get("version"), bool):
        raise CheckError(f"{path}: version must be 0")
    if "name" in data and (not isinstance(data["name"], str) or not 1 <= len(data["name"]) <= 64):
        raise CheckError(f"{path}: bad name")
    neurons = data.get("neurons")
    if not isinstance(neurons, list) or not neurons:
        raise CheckError(f"{path}: neurons must be a non-empty array")
    neurons = [_resolve(n, i, base) for i, n in enumerate(neurons)]
    data = dict(data)
    data["neurons"] = neurons
    ids = {}
    for i, n in enumerate(neurons):
        kind = _neuron(n, i)
        if n["id"] in ids:
            raise CheckError(f"{path}: duplicate id {n['id']}")
        ids[n["id"]] = (n, kind)
    links = data.get("links", [])
    if not isinstance(links, list):
        raise CheckError(f"{path}: links must be an array")
    seen = set()
    for l in links:
        if not isinstance(l, dict) or set(l) != {"from", "to"} or not all(isinstance(l[k], str) for k in l):
            raise CheckError(f"{path}: bad link")
        if l["from"] not in ids or l["to"] not in ids:
            raise CheckError(f"{path}: link names an unknown id")
        if (l["from"], l["to"]) in seen:
            raise CheckError(f"{path}: duplicate link")
        seen.add((l["from"], l["to"]))
        if ids[l["to"]][1] == "source":
            raise CheckError(f"{path}: link into a source")
    if ("entry" in data) != ("exit" in data):
        raise CheckError(f"{path}: entry and exit go together")
    for k in ("entry", "exit"):
        if k in data and data[k] not in ids:
            raise CheckError(f"{path}: {k} names an unknown id")
    budget = data.get("budget")
    if "budget" in data:
        _units(budget, "budget")
    for n, kind in ids.values():
        if "cost" in n:
            if budget is None:
                raise CheckError(f"{path}: cost without a budget names no unit")
            for u in n["cost"]:
                if u not in budget:
                    raise CheckError(f"{path}: cost names unit {u} the budget lacks")
        elif budget is not None and kind != "source":
            raise CheckError(f"{path}: neuron {n['id']} has no cost under a budget")
    if "ground" in data:
        _ground(data["ground"], ids, path)
    # a cycle in which no neuron has maxHops
    nohop = {i for i, (n, k) in ids.items() if "maxHops" not in n}
    adj = {i: [l["to"] for l in links if l["from"] == i and l["to"] in nohop] for i in nohop}
    state = {}

    def dfs(u):
        state[u] = 1
        for v in adj[u]:
            if state.get(v) == 1 or (v not in state and dfs(v)):
                return True
        state[u] = 2
        return False

    for u in nohop:
        if u not in state and dfs(u):
            raise CheckError(f"{path}: a cycle with no maxHops")
    inner = {}
    for i, (n, kind) in ids.items():
        if kind == "network":
            sub, _ = load(os.path.join(base, n["network"]), _stack + (path,))
            if "entry" not in sub:
                raise CheckError(f"{n['network']}: composed file needs entry and exit")
            if any(k == "source" for _, k in ((x, _kind(x)) for x in sub["neurons"])):
                raise CheckError(f"{n['network']}: composed file may not contain a source")
            if any(l["from"] == sub["exit"] for l in sub.get("links", [])):
                raise CheckError(f"{n['network']}: exit has outgoing links")
            if any("effect" in x for x in sub["neurons"]):
                raise CheckError(f"{n['network']}: composed file may not contain an effect neuron")
            if "ground" in sub:
                raise CheckError(f"{n['network']}: composed file may not declare ground")
            # the fold (2.7, decision 0016): the composed neuron's may covers what its inside declares
            mine = n.get("may", {})
            for x in sub["neurons"]:
                for member in MAY:
                    for v in x.get("may", {}).get(member, []):
                        if v not in mine.get(member, []):
                            raise CheckError(f"{path}: the may of {i} does not cover its inside: {x['id']} may {member} {v!r}")
            inner[i] = sub
    data = dict(data)
    data["_inner"] = inner
    data["_base"] = base
    data["_path"] = path
    return data, base


def _kind(n):
    return next(k for k in KINDS if k in n)
