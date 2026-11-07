# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Tests del frontend de nodos (Python puro)."""

from __future__ import annotations

import json
import unittest

from tlaxtrelom_tools.graph import (
    Graph,
    T,
    add,
    neural_read,
    select,
    vec3_compose,
    vec3_scale,
)
from tlaxtrelom_tools.ir import HASH_PREFIX, verify


class GraphTests(unittest.TestCase):
    def test_lowering_is_deterministic_and_verifiable(self) -> None:
        def build() -> dict:
            with Graph("locomotion") as graph:
                speed = graph.input("speed", T.F32)
                forward = graph.call(
                    vec3_compose,
                    x=graph.const(0.0),
                    y=graph.const(0.0),
                    z=graph.const(1.0),
                )
                velocity = graph.call(vec3_scale, v=forward, s=speed)
                graph.output("velocity", velocity)
            return graph.lower()

        first = build()
        second = build()
        self.assertEqual(first["hash"], second["hash"])
        self.assertTrue(first["hash"].startswith(HASH_PREFIX))
        verify(first)
        self.assertEqual(len(first["ops"]), 2)

    def test_explicit_i32_to_f32_promotion(self) -> None:
        with Graph("promotion") as graph:
            value = graph.input("value", T.I32)
            out = graph.call(add, a=value, b=graph.const(1.0))
            graph.output("out", out)
        hir = graph.lower()
        casts = [op for op in hir["ops"] if op["op"] == "cast"]
        self.assertEqual(len(casts), 1)
        self.assertEqual(casts[0]["type"], T.F32)
        verify(hir)

    def test_incompatible_types_are_rejected(self) -> None:
        graph = Graph("bad_types")
        text = graph.const(True)
        number = graph.const(1.0)
        with self.assertRaises(TypeError):
            graph.call(add, a=text, b=number)

    def test_unknown_input_port_is_rejected(self) -> None:
        graph = Graph("bad_port")
        with self.assertRaises(TypeError):
            graph.call(add, a=graph.const(1.0), b=graph.const(2.0), c=graph.const(3.0))

    def test_missing_input_port_is_rejected(self) -> None:
        graph = Graph("missing")
        with self.assertRaises(TypeError):
            graph.call(add, a=graph.const(1.0))

    def test_deterministic_graph_rejects_neural_effect(self) -> None:
        with self.assertRaises(ValueError):
            with Graph("neural_illegal") as graph:
                value = graph.call(neural_read, channel=graph.const(0))
                graph.output("value", value)
            graph.lower()

    def test_select_inherits_value_type(self) -> None:
        with Graph("choose") as graph:
            cond = graph.input("cond", T.BOOL)
            out = graph.call(
                select,
                cond=cond,
                a=graph.const(1.5),
                b=graph.const(2.5),
            )
            graph.output("out", out)
        hir = graph.lower()
        self.assertEqual(hir["outputs"][0]["type"], T.F32)
        verify(hir)

    def test_graph_without_outputs_is_rejected(self) -> None:
        graph = Graph("empty")
        with self.assertRaises(ValueError):
            graph.lower()

    def test_duplicate_output_is_rejected(self) -> None:
        graph = Graph("duplicate")
        value = graph.const(1.0)
        graph.output("out", value)
        with self.assertRaises(ValueError):
            graph.output("out", value)

    def test_invalid_module_name_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            Graph("nombre con espacios")

    def test_hash_changes_when_payload_changes(self) -> None:
        hir = Graph("no_outputs")
        with self.assertRaises(ValueError):
            hir.lower()

        with Graph("a") as graph:
            graph.output("out", graph.const(1.0))
        first = graph.lower()
        with Graph("b") as graph:
            graph.output("out", graph.const(2.0))
        second = graph.lower()
        self.assertNotEqual(first["hash"], second["hash"])
        self.assertEqual(json.loads(json.dumps(first)), first)


if __name__ == "__main__":
    unittest.main()
