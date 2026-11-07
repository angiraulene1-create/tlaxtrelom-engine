# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Tests del frontend de bloques."""

from __future__ import annotations

import unittest

from tlaxtrelom_tools.blocks import Script
from tlaxtrelom_tools.graph import T
from tlaxtrelom_tools.ir import verify


class BlocksTests(unittest.TestCase):
    def test_sequence_and_repeat_lower_to_dag(self) -> None:
        script = Script("turret")
        script.emit(1)
        script.repeat(3, lambda: script.emit(2))
        hir = script.finish()
        verify(hir)
        emitted = [op for op in hir["ops"] if op["op"] == "flow.emit"]
        self.assertEqual(len(emitted), 4)
        seqs = [op for op in hir["ops"] if op["op"] == "flow.seq"]
        self.assertEqual(len(seqs), 3)
        self.assertEqual(hir["outputs"][0]["name"], "flow")

    def test_zero_repeat_is_allowed(self) -> None:
        script = Script("idle")
        script.repeat(0, lambda: None)
        script.emit(7)
        verify(script.finish())

    def test_negative_repeat_is_rejected(self) -> None:
        script = Script("bad")
        with self.assertRaises(ValueError):
            script.repeat(-1, lambda: None)

    def test_repeat_limit_prevents_unbounded_unroll(self) -> None:
        script = Script("too_big")
        with self.assertRaises(ValueError):
            script.repeat(65, lambda: None)

    def test_variables_are_ssa_and_tracked(self) -> None:
        script = Script("vars")
        initial = script.var("speed", 0.0)
        self.assertIs(script.get("speed"), initial)
        replacement = script.graph.const(2.0, type=T.F32)
        script.set("speed", replacement)
        self.assertIs(script.get("speed"), replacement)
        script.emit(1)
        verify(script.finish())

    def test_unknown_variable_is_rejected(self) -> None:
        script = Script("vars")
        with self.assertRaises(KeyError):
            script.get("missing")

    def test_duplicate_variable_is_rejected(self) -> None:
        script = Script("vars")
        script.var("x", 1)
        with self.assertRaises(ValueError):
            script.var("x", 2)

    def test_if_builds_select_node(self) -> None:
        script = Script("choice")
        cond = script.graph.const(True)
        out = script.if_(cond, lambda: script.graph.const(1.0), lambda: script.graph.const(2.0))
        self.assertEqual(out.type, T.F32)
        script.graph.output("chosen", out)
        hir = script.finish()
        self.assertTrue(any(op["op"] == "math.select" for op in hir["ops"]))
        verify(hir)

    def test_script_is_deterministic(self) -> None:
        def build() -> dict:
            script = Script("turret")
            script.emit(1)
            script.repeat(2, lambda: script.emit(2))
            return script.finish()

        self.assertEqual(build()["hash"], build()["hash"])


if __name__ == "__main__":
    unittest.main()
