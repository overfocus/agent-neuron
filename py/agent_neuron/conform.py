"""The fixture runner of CONFORMANCE sections 2, 3, 6 and 8."""
import copy
import json
import os
import sys

from .engine import Harness, Refused, Run

TOP = {"fixture", "description", "network", "notes", "finished", "streamFails", "pending", "pingEvery",
       "refuses", "inputs", "script", "observer", "expect", "expectWithout", "independent"}


def find_fixtures(root):
    out = []
    for d, dirs, files in os.walk(root):
        dirs.sort()
        if "fixture.json" in files:
            out.append(d)
    return sorted(out, key=lambda d: os.path.relpath(d, root))


def normalise(events):
    ids = {}

    def num(v):
        if v not in ids:
            ids[v] = f"#{len(ids) + 1}"
        return ids[v]

    out = []
    for ev in events:
        ev = dict(ev)
        for f in ("id", "trace", "replyTo", "cause", "trigger"):
            if f in ev:
                ev[f] = num(ev[f])
        out.append(ev)
    return out


def first_diff(actual, expected):
    for i in range(max(len(actual), len(expected))):
        if i >= len(actual):
            return f"event {i}: missing, expected {json.dumps(expected[i])}"
        if i >= len(expected):
            return f"event {i}: unexpected {json.dumps(actual[i])}"
        a, e = actual[i], dict(expected[i])
        if e.get("via") == "emit":
            e.pop("via")
        a = dict(a)
        if e.get("type") in ("error", "unknown") and "message" in e and isinstance(a.get("message"), str):
            if not a["message"].startswith(e["message"]):
                return f"event {i}: message {a['message']!r} does not start with {e['message']!r}"
            a["message"] = e["message"]
        if a != e:
            return f"event {i}:\n    actual   {json.dumps(a)}\n    expected {json.dumps(e)}"
    return None


def execute(fx, path, observer):
    """One run: returns (events, stream copies).  Raises Refused or Harness."""
    base = os.path.dirname(path)
    net = os.path.join(base, fx.get("network", "network.json"))
    copies = []

    def consumer(ev):
        copies.append(copy.deepcopy(ev))
        ev["t"] = -1
        ev["poison"] = True
        if fx.get("streamFails"):
            raise RuntimeError("consumer fails")

    run = Run(net, fx["inputs"], fx.get("script") or {}, fx.get("pingEvery", 10000), observer, consumer)
    events = run.execute()
    return events, copies


def filt(events, fx):
    out = []
    for ev in events:
        if ev["type"] == "note" and not fx.get("notes"):
            continue
        if ev["type"] == "finished" and not fx.get("finished"):
            continue
        out.append(ev)
    return out


def check_fixture(fx, path):
    """Return None on pass, or the first difference."""
    unknown = set(fx) - TOP
    if unknown:
        return f"unknown top-level field {sorted(unknown)[0]}"
    obs = fx.get("observer")
    try:
        events, copies = execute(fx, path, obs)
    except Refused as e:
        if fx.get("refuses"):
            return None if not fx["expect"] else "refused, but expect is not empty"
        return f"engine refused to start: {e}"
    except Harness as e:
        return f"harness failure: {e}"
    if fx.get("refuses"):
        return "engine started, but the fixture expects a refusal"
    if copies != events:
        return "the live stream differs from the events: " + (first_diff(copies, events) or "")
    d = first_diff(normalise(filt(events, fx)), fx["expect"])
    if d:
        return d
    if obs is not None:
        try:
            plain, copies2 = execute(fx, path, [])
        except (Refused, Harness) as e:
            return f"run without the observer: {e}"
        if copies2 != plain:
            return "the live stream differs from the events (run without the observer)"
        rules = bool(obs)
        if "expectWithout" in fx:
            d = first_diff(normalise(filt(plain, fx)), fx["expectWithout"])
            if d:
                return "without the observer: " + d
        elif not rules:
            d = first_diff(normalise(filt(plain, fx)), fx["expect"])
            if d:
                return "without the observer: " + d
        idx = next((i for i, e in enumerate(events) if e["type"] == "note" and e.get("what") == "inject"), None)
        if idx is not None:
            if events[:idx] != plain[:idx] and normalise(events[:idx]) != normalise(plain[:idx]):
                return "events before the first inject note differ between the two runs"
        if fx.get("independent") or idx is None:
            own = set()
            for i, e in enumerate(events):
                if e["type"] == "note" and e.get("what") == "inject":
                    own.add(events[i + 1]["trace"])
            a = normalise([e for e in events if e["type"] != "note" and e.get("trace") not in own])
            b = normalise([e for e in plain if e["type"] != "note"])
            d = first_diff(a, b)
            if d:
                return "removing the observer's traces differs from the run without it: " + d
    return None


def conform(root, only=None, out=sys.stdout):
    fixtures = find_fixtures(root)
    if not fixtures:
        print(f"no fixtures under {root}", file=sys.stderr)
        return 2
    passed = failed = pending = 0
    for d in fixtures:
        name = os.path.relpath(d, root).replace(os.sep, "/")
        if only and only not in name:
            continue
        path = os.path.join(d, "fixture.json")
        try:
            with open(path, encoding="utf-8") as fh:
                fx = json.load(fh)
        except ValueError as e:
            print(f"fail {name}\n  {e}", file=out)
            failed += 1
            continue
        if "pending" in fx:
            print(f"pending {name}: {fx['pending']}", file=out)
            pending += 1
            continue
        try:
            diff = check_fixture(fx, path)
        except Exception as e:  # a bug in this engine is a failed fixture, not a crash of the runner
            import traceback
            diff = "engine error: " + "".join(traceback.format_exception_only(type(e), e)).strip() + "\n" + "".join(traceback.format_tb(e.__traceback__)[-3:])
        if diff is None:
            print(f"pass {name}", file=out)
            passed += 1
        else:
            print(f"fail {name}", file=out)
            for line in diff.split("\n"):
                print("  " + line, file=out)
            failed += 1
    print(f"{passed} passed, {failed} failed, {pending} pending", file=out)
    return 1 if failed else 0
