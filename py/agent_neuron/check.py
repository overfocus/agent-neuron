"""Network file loading and the errors of SPEC 2.7 that stop an engine from starting.

Standard library only. Written from SPEC section 2 and spec/network.schema.json's
description (closed schema, literal values), not from the TypeScript checker.
"""
import json
import os
import re

ID_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$")
ENV_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
KINDS = ("source", "run", "serve", "http", "human", "network")
COMMON = {"id", "description"}
OPTS = {
    "source": set(),
    "run": {"threshold", "within", "maxHops", "timeout", "env", "cwd", "cost", "effect"},
    "serve": {"threshold", "within", "maxHops", "timeout", "env", "cwd", "cost", "effect"},
    "http": {"threshold", "within", "maxHops", "timeout", "cost", "effect"},
    "human": {"threshold", "within", "maxHops", "timeout", "cost", "effect"},
    "network": {"maxHops", "timeout", "cost"},
}
TOP = {"$schema", "version", "name", "description", "neurons", "links", "entry", "exit", "budget"}
MAX_AMOUNT = 2**53 - 1


class CheckError(Exception):
    pass


def _isint(v):
    return isinstance(v, int) and not isinstance(v, bool)


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
        if f in n and (not _isint(n[f]) or not 1 <= n[f] <= 3600000):
            raise CheckError(f"neurons[{i}]: {f} must be an integer from 1 to 3600000")
    if "maxHops" in n and (not _isint(n["maxHops"]) or n["maxHops"] < 0):
        raise CheckError(f"neurons[{i}]: maxHops must be an integer >= 0")
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
            inner[i] = sub
    data = dict(data)
    data["_inner"] = inner
    data["_base"] = base
    data["_path"] = path
    return data, base


def _kind(n):
    return next(k for k in KINDS if k in n)
