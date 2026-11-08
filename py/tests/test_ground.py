"""Unit tests beside the Python engine, written from SPEC 4.5 (decision 0012) and the ground fixtures.

Standard library only (unittest).  Run from the repo root:

    PYTHONPATH=py python3 -B -m unittest discover -s py/tests -p "test_*.py"

The refusals class enumerates every refusal of 0012 item 5; the deposit classes
enumerate the ground event's fields and its place in the event order; the draw
classes pin the generator's words and the draws the fixtures quote.
"""
import json
import math
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from agent_neuron.check import CheckError, load
from agent_neuron.engine import Run


def _write(tmp, net, name="network.json"):
    path = os.path.join(tmp, name)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(net, fh)
    return path


def _net(tmp, nodes, links, ground=None, inner=None, name="network.json"):
    """A network file with the made-up ids the tests use: nodes is [(id, kind)]."""
    net = {"version": 0, "name": name, "neurons": [], "links": links}
    for nid, kind in nodes:
        n = {"id": nid, kind: ["neuron-" + nid] if kind in ("run", "serve") else True}
        net["neurons"].append(n)
    net["links"] = [{"from": a, "to": b} for a, b in links]
    if ground is not None:
        net["ground"] = ground
    path = _write(tmp, net, name)
    if inner is not None:  # a composed neuron: node (id, "network") reads name inner
        for nid, kind in nodes:
            if kind == "network":
                _write(tmp, inner, nid + ".inner.json")
    return path


def _run(path, inputs, script):
    """Run the network; `finished` events are stripped, the tests enumerate the work events."""
    return [e for e in Run(path, inputs, script).execute() if e["type"] != "finished"]


SRC = [("s", "source")]


class TmpCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="agent-neuron-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)


class TestRefusals(TmpCase):
    """Every refusal of 0012 item 5, by message."""

    def refused(self, net, fragment, name="network.json"):
        path = _net(self.tmp, **net, name=name)
        with self.assertRaises(CheckError) as cm:
            load(path)
        self.assertIn(fragment, str(cm.exception))
        with self.assertRaises(CheckError):
            load(path)  # twice, in case a run's consumer caches anything

    def test_ground_must_be_an_object(self):
        self.refused({"nodes": SRC, "links": [], "ground": "trail"}, "ground must be an object")

    def test_ground_unknown_field(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "salt": 3}}, "unknown field salt")

    def test_deposit_unknown_field(self):
        dep = {"on": "place", "add": "amount", "deep": "x"}
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": dep}}, "deposit: unknown field deep")

    def test_deposit_without_on(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": {"add": "amount"}}}, "deposit needs a on")

    def test_deposit_without_add(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": {"on": "place"}}}, "deposit needs a add")

    def test_deposit_names_are_names(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": {"on": 5, "add": "amount"}}}, "deposit.on")

    def test_by_names_an_unknown_neuron(self):
        dep = {"on": "place", "add": "amount", "by": "ghost"}
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": dep}}, "names no declared neuron")

    def test_negative_base(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "base": -1}}, "base must be a number from 0 up")

    def test_base_must_be_a_number(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "base": "x"}}, "base")

    def test_negative_evaporate(self):
        dep = {"on": "place", "add": "amount"}
        self.refused({"nodes": SRC, "links": [], "ground": {"deposit": dep, "evaporate": -0.5}}, "evaporate")

    def test_power_of_zero(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "power": 0}}, "power must be a number above 0")

    def test_negative_power(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "power": -2}}, "power must be a number above 0")

    def test_select_must_be_true(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": False}}, "select must be true")

    def test_negative_seed(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "seed": -1}}, "seed must be a whole number from 0 up")

    def test_seed_must_be_an_integer(self):
        self.refused({"nodes": SRC, "links": [], "ground": {"select": True, "seed": 1.5}}, "seed must be a whole number from 0 up")

    def test_inner_ground_refused(self):
        nodes = SRC + [("box", "network")]
        inner = {"version": 0, "name": "inner", "entry": "e", "exit": "e",
                 "neurons": [{"id": "e", "run": ["neuron-e"]}],
                 "ground": {"select": True, "seed": 1}}
        path = _net(self.tmp, nodes, [("s", "box")], inner=inner, name="box.inner.json")
        main = {"version": 0, "name": "main", "neurons": [{"id": "s", "source": True}, {"id": "box", "network": "box.inner.json"}],
                "links": [{"from": "s", "to": "box"}]}
        path = _write(self.tmp, main)
        with self.assertRaises(CheckError) as cm:
            load(path)
        self.assertIn("may not declare ground", str(cm.exception))

    def test_a_valid_ground_loads(self):
        dep = {"on": "place", "add": "amount", "by": "mid"}
        g = {"deposit": dep, "select": True, "base": 2, "power": 1.5, "seed": 7, "evaporate": 0.5}
        nodes = SRC + [("mid", "run")]
        path = _net(self.tmp, nodes, [("s", "mid")], ground=g)
        data, _ = load(path)
        self.assertEqual(data["ground"], g)
        self.assertEqual(data["ground"], load(path)[0]["ground"])


class TestDeposit(TmpCase):
    """A deposit writes one place: the event, its adjacency, ageing at the write, reads that never mutate."""

    GROUND = {"deposit": {"on": "place", "add": "amount"}, "evaporate": 0.5}
    INPUTS = [{"at": 0, "source": "s", "payload": {"place": "p1", "amount": 7}},
              {"at": 400, "source": "s", "payload": {"place": "p1", "amount": 0.3}}]
    SCRIPT = {"mid": {"calls": [{"when": {"place": "p1", "amount": 7}, "reply": {"answer": {"place": "p1", "amount": 7}}},
                                {"when": {"place": "p1", "amount": 0.3}, "reply": {"answer": {"place": "p1", "amount": 0.3}}}]},
              "end": {"calls": [{"reply": {"quiet": True}}, {"reply": {"quiet": True}}]}}

    def setUp(self):
        super().setUp()
        self.path = _net(self.tmp, SRC + [("mid", "run"), ("end", "run")],
                         [("s", "mid"), ("mid", "end")], ground=self.GROUND)

    def test_ground_event_adjacency_and_shape(self):
        events = _run(self.path, self.INPUTS, self.SCRIPT)
        types = [e["type"] for e in events]
        self.assertEqual(types, ["signal", "answer", "ground", "signal", "quiet",
                                 "signal", "answer", "ground", "signal", "quiet"])
        for i, answer_i in ((2, 1), (7, 6)):
            g = events[i]
            # the enumerated fields of a ground event (7.2): place, v, cause, and no signal fields
            self.assertEqual(set(g), {"type", "t", "place", "v", "cause"})
            self.assertEqual(g["place"], "p1")
            self.assertEqual(g["cause"], events[answer_i]["id"])  # the depositing answer's id
            self.assertEqual(events[answer_i]["type"], "answer")
        self.assertEqual(events[2]["t"], 0)
        self.assertEqual(events[7]["t"], 400)

    def test_the_write_ages_the_old_value_and_adds_last(self):
        events = _run(self.path, self.INPUTS, self.SCRIPT)
        self.assertEqual(events[2]["v"], 7.0)  # the first write stores add alone
        expected = 7 * math.exp(-(0.5 * 400 / 1000)) + 0.3  # the SPEC's order, same as the engine's
        self.assertEqual(events[7]["v"], expected)

    def test_a_read_never_mutates_the_entry(self):
        run = Run(self.path, self.INPUTS, self.SCRIPT)
        run.execute()
        entry = run.places["p1"]
        self.assertEqual(entry[1], 400)  # the instant of the last write
        run.sched.now = 900
        self.assertEqual(run.eff_value("p1"), entry[0] * math.exp(-(0.5 * (900 - 400) / 1000)))
        self.assertEqual(run.places["p1"], entry)  # the read left the entry where it was

    def test_by_restricts_the_writer(self):
        dep = {"on": "place", "add": "amount", "by": "mid"}
        path = _net(self.tmp, SRC + [("s2", "source"), ("mid", "run"), ("other", "run"), ("end", "run")],
                    [("s", "mid"), ("s2", "other"), ("mid", "end"), ("other", "end")], ground={"deposit": dep})
        script = {"mid": {"calls": [{"when": 1, "reply": {"answer": {"place": "p1", "amount": 7}}}]},
                  "other": {"calls": [{"when": 2, "reply": {"answer": {"place": "p1", "amount": 9}}}]},
                  "end": {"calls": [{"when": {"place": "p1", "amount": 7}, "reply": {"quiet": True}},
                                    {"when": {"place": "p1", "amount": 9}, "reply": {"quiet": True}}]}}
        events = _run(path, [{"at": 0, "source": "s", "payload": 1}, {"at": 10, "source": "s2", "payload": 2}], script)
        grounds = [e for e in events if e["type"] == "ground"]
        self.assertEqual(len(grounds), 1)  # only the neuron `by` names deposits
        self.assertEqual(grounds[0]["v"], 7.0)

    def test_a_reply_without_the_fields_deposits_nothing(self):
        events = _run(self.path, [{"at": 0, "source": "s", "payload": "poke"}],
                      {"mid": {"calls": [{"reply": {"answer": "poke"}}]},
                       "end": {"calls": [{"reply": {"quiet": True}}]}})
        self.assertEqual([e["type"] for e in events], ["signal", "answer", "signal", "quiet"])

    def test_the_amount_must_be_a_finite_number(self):
        for bad in ("many", True, None):
            with self.subTest(bad=bad):
                events = _run(self.path, [{"at": 0, "source": "s", "payload": {"place": "p1", "amount": bad}}],
                              {"mid": {"calls": [{"reply": {"answer": {"place": "p1", "amount": bad}}}]},
                               "end": {"calls": [{"reply": {"quiet": True}}]}})
                self.assertNotIn("ground", [e["type"] for e in events])

    def test_select_false_writes_and_routes_all(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2")],
                    ground={"deposit": {"on": "place", "add": "amount"}})
        script = {"mid": {"calls": [{"reply": {"answer": {"place": "a2", "amount": 4}}}]},
                  "a1": {"calls": [{"reply": {"quiet": True}}]}, "a2": {"calls": [{"reply": {"quiet": True}}]}}
        events = _run(path, [{"at": 0, "source": "s", "payload": 1}], script)
        self.assertEqual([e["type"] for e in events], ["signal", "answer", "ground", "signal", "quiet", "quiet"])
        self.assertEqual(events[3]["to"], ["a1", "a2"])  # no select: the v0.1 rule forwards on every out-link
        self.assertEqual(events[2]["v"], 4.0)


class TestDrawing(TmpCase):
    """The pinned generator's words, and the draws the fixtures quote."""

    def test_the_words_match_the_pinned_generator(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2")], ground={"select": True, "seed": 8})
        run = Run(path, [], {})
        words = [run.draw_word() for _ in range(3)]
        # the fixture quotes 0.1563, 0.6252, and 0.3007 for seed 8
        for got, want in zip(words, (0.1563, 0.6252, 0.3007)):
            self.assertAlmostEqual(got, want, delta=5e-5)

    def test_the_state_is_32_bit(self):
        path = _net(self.tmp, SRC + [("mid", "run")], [("s", "mid")], ground={"select": True, "seed": 7})
        run = Run(path, [], {})
        for _ in range(50):
            u = run.draw_word()
            self.assertGreaterEqual(u, 0.0)
            self.assertLess(u, 1.0)
            self.assertEqual(run.rng & 0xFFFFFFFF, run.rng)

    def test_equal_weights_follow_the_seed_in_declaration_order(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run"), ("a3", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2"), ("mid", "a3")],
                    ground={"select": True, "seed": 35})
        script = {"mid": {"calls": [{"reply": {"answer": 11}}, {"reply": {"answer": 12}}, {"reply": {"answer": 13}}]},
                  "a1": {"calls": [{"when": 11, "reply": {"quiet": True}}]},
                  "a2": {"calls": [{"when": 12, "reply": {"quiet": True}}]},
                  "a3": {"calls": [{"when": 13, "reply": {"quiet": True}}]}}
        inputs = [{"at": at, "source": "s", "payload": n} for at, n in ((0, 11), (10, 12), (20, 13))]
        events = _run(path, inputs, script)
        forwards = [e for e in events if e["type"] == "signal" and e["from"] == "mid"]
        self.assertEqual([e["to"] for e in forwards], [["a1"], ["a2"], ["a3"]])  # u 0.0075, 0.5055, 0.7220

    def test_a_reply_naming_to_consumes_no_word(self):
        path = _net(self.tmp, SRC + [("s2", "source"), ("n1", "serve"), ("n2", "run"), ("c1", "run"), ("c2", "run"), ("c3", "run")],
                    [("s", "n1"), ("s2", "n2"), ("n1", "c1"), ("n2", "c2"), ("n2", "c3")],
                    ground={"select": True, "seed": 24})
        script = {"n1": {"calls": [{"when": {"kind": "named"}, "reply": {"answer": {"kind": "named"}, "to": ["c1"]}}]},
                  "n2": {"calls": [{"when": {"kind": "draw"}, "reply": {"answer": {"kind": "draw"}}}]},
                  "c1": {"calls": [{"when": {"kind": "named"}, "reply": {"quiet": True}}]},
                  "c2": {"calls": [{"when": {"kind": "draw"}, "reply": {"quiet": True}}]}}
        inputs = [{"at": 0, "source": "s", "payload": {"kind": "named"}},
                  {"at": 10, "source": "s2", "payload": {"kind": "draw"}}]
        events = _run(path, inputs, script)
        forwards = [e for e in events if e["type"] == "signal" and e["hops"] == 2]
        self.assertEqual(forwards[0]["to"], ["c1"])  # the named `to` routes as 4.4 says
        self.assertEqual(forwards[1]["to"], ["c2"])  # the omitted reply drew word 1 (0.3320), not word 2 (0.9638)

    def test_a_replys_own_deposit_counts_in_its_draw(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run"), ("a3", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2"), ("mid", "a3")],
                    ground={"select": True, "base": 0, "deposit": {"on": "place", "add": "amount"}})
        script = {"mid": {"calls": [{"when": {"place": "a2", "amount": 3},
                                     "reply": {"answer": {"place": "a2", "amount": 3}}}]},
                  "a2": {"calls": [{"when": {"place": "a2", "amount": 3}, "reply": {"quiet": True}}]}}
        events = _run(path, [{"at": 0, "source": "s", "payload": {"place": "a2", "amount": 3}}], script)
        self.assertEqual([e["type"] for e in events], ["signal", "answer", "ground", "signal", "quiet"])
        self.assertEqual(events[2]["v"], 3.0)  # the deposit is written before the draw weighs it
        self.assertEqual(events[3]["to"], ["a2"])  # weight 3 against two weights of 0

    def test_a_dead_trail_forwards_nothing(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2")],
                    ground={"select": True, "base": 0, "deposit": {"on": "place", "add": "amount"}})
        script = {"mid": {"calls": [{"when": {"place": "a1", "amount": 3}, "reply": {"answer": {"place": "a1", "amount": 3}}},
                                    {"when": {"place": "a1", "amount": -3}, "reply": {"answer": {"place": "a1", "amount": -3}}}]},
                  "a1": {"calls": [{"when": {"place": "a1", "amount": 3}, "reply": {"quiet": True}}]}}
        inputs = [{"at": 0, "source": "s", "payload": {"place": "a1", "amount": 3}},
                  {"at": 10, "source": "s", "payload": {"place": "a1", "amount": -3}}]
        events = _run(path, inputs, script)
        self.assertEqual(events[2]["v"], 3.0)
        self.assertEqual(events[3]["to"], ["a1"])
        self.assertEqual(events[7]["v"], 0.0)  # the trail marked dead: 3·exp(0) − 3
        self.assertEqual(events[8:], [])  # reported, and nothing forwarded

    def test_without_ground_the_omitted_reply_takes_every_link(self):
        path = _net(self.tmp, SRC + [("mid", "run"), ("a1", "run"), ("a2", "run"), ("a3", "run")],
                    [("s", "mid"), ("mid", "a1"), ("mid", "a2"), ("mid", "a3")])
        script = {"mid": {"calls": [{"reply": {"answer": 1}}]},
                  "a1": {"calls": [{"reply": {"quiet": True}}]}, "a2": {"calls": [{"reply": {"quiet": True}}]},
                  "a3": {"calls": [{"reply": {"quiet": True}}]}}
        events = _run(path, [{"at": 0, "source": "s", "payload": 1}], script)
        self.assertEqual([e["to"] for e in events if e["type"] == "signal" and e["hops"] == 2], [["a1", "a2", "a3"]])
        self.assertEqual([e["type"] for e in events if e["type"] == "ground"], [])


if __name__ == "__main__":
    unittest.main()
