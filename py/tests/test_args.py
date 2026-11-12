"""`args` on an entry that names a neuron's file (SPEC 2.10, decision 0022), in the Python loader.

Standard library only (unittest).  Run from the repo root:

    PYTHONPATH=py python3 -B -m unittest discover -s py/tests -p "test_*.py"

The fixtures script every program, so they cannot see a command: these tests read the
neuron the loader hands the engine, which is the one written inline with the longer command.
"""
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from agent_neuron.check import CheckError, load

NAMED = {"neuron": "kit/a.neuron.json", "args": ["--apply", "two words", "--apply"]}


class TestArgs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="agent-neuron-args-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        os.mkdir(os.path.join(self.tmp, "kit"))

    def write(self, a, own=None):
        """A network whose neuron `a` is `a`, with `own` as kit/a.neuron.json; the network file's path."""
        if own is not None:
            with open(os.path.join(self.tmp, "kit", "a.neuron.json"), "w", encoding="utf-8") as fh:
                json.dump({"version": 0, **own}, fh)
        net = {"version": 0, "neurons": [{"id": "s", "source": True}, {"id": "a", **a}], "links": [{"from": "s", "to": "a"}]}
        path = os.path.join(self.tmp, "net.json")
        with open(path, "w", encoding="utf-8") as fh:
            json.dump(net, fh)
        return path

    def refused(self, a, own, fragment):
        with self.assertRaises(CheckError) as cm:
            load(self.write(a, own))
        self.assertIn(fragment, str(cm.exception))

    def test_appended_after_the_files_own_arguments(self):
        for kind in ("run", "serve"):
            data, _ = load(self.write(NAMED, {kind: ["neuron-a", "own"], "timeout": 100}))
            a = data["neurons"][1]
            self.assertEqual(a[kind], ["neuron-a", "own", "--apply", "two words", "--apply"])
            self.assertNotIn("args", a)
            self.assertNotIn("neuron", a)
            self.assertEqual(a["timeout"], 100)

    def test_without_args_the_command_is_the_files(self):
        data, _ = load(self.write({"neuron": "kit/a.neuron.json"}, {"run": ["neuron-a", "own"]}))
        self.assertEqual(data["neurons"][1]["run"], ["neuron-a", "own"])

    def test_refused_beside_a_file_with_no_command(self):
        for own in ({"http": "http://localhost:1/"}, {"human": True}):
            self.refused(NAMED, own, "args adds to a command")

    def test_refused_on_an_inline_neuron(self):
        for inline in ({"run": ["neuron-a"]}, {"serve": ["neuron-a"]}, {"http": "http://localhost:1/"}, {"human": True}, {"source": True}):
            self.refused({**inline, "args": ["--apply"]}, None, "unknown field args")

    def test_refused_when_not_a_non_empty_array_of_literals(self):
        own = {"run": ["neuron-a"]}
        for bad in ([], "x"):
            self.refused({**NAMED, "args": bad}, own, "non-empty array")
        for bad in ([1], ["${HOME}"], ["a", "{{b}}"]):
            self.refused({**NAMED, "args": bad}, own, "not a literal string")

    def test_refused_in_the_neuron_file(self):
        self.refused({"neuron": "kit/a.neuron.json"}, {"run": ["neuron-a"], "args": ["--apply"]}, "args is not the neuron's own")


if __name__ == "__main__":
    unittest.main()
