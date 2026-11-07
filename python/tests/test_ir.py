# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Tests de la forma canónica y el validador de HIR."""

from __future__ import annotations

import copy
import json
import unittest

from tlaxtrelom_tools.ir import (
    HASH_PREFIX,
    IrError,
    canonical_bytes,
    from_bytes,
    hash_hir,
    seal,
    to_bytes,
    verify,
)


def sample_hir() -> dict:
    return {
        "schema": "tlaxtrelom.ir/1",
        "module": "sample",
        "entry": "main",
        "deterministic": True,
        "values": [
            {"id": 0, "type": "f32", "kind": "const", "value": 1.0},
            {"id": 1, "type": "f32", "kind": "op"},
        ],
        "ops": [
            {
                "id": 1,
                "op": "math.add",
                "args": [0, 0],
                "type": "f32",
                "effects": ["pure"],
            }
        ],
        "outputs": [{"name": "out", "value": 1, "type": "f32"}],
    }


class IrTests(unittest.TestCase):
    def test_seal_is_idempotent(self) -> None:
        sealed = seal(sample_hir())
        self.assertTrue(sealed["hash"].startswith(HASH_PREFIX))
        self.assertEqual(sealed, seal(sealed))
        verify(sealed)

    def test_integral_floats_and_ints_hash_equal(self) -> None:
        a = sample_hir()
        b = sample_hir()
        b["values"][0]["value"] = 1
        self.assertEqual(hash_hir(a), hash_hir(b))

    def test_tampering_is_detected(self) -> None:
        sealed = seal(sample_hir())
        tampered = copy.deepcopy(sealed)
        tampered["outputs"][0]["name"] = "other"
        with self.assertRaises(IrError):
            verify(tampered)

    def test_bad_schema_is_rejected(self) -> None:
        payload = sample_hir()
        payload["schema"] = "tlaxtrelom.ir/999"
        with self.assertRaises(IrError):
            seal(payload)

    def test_dangling_argument_is_rejected(self) -> None:
        payload = sample_hir()
        payload["ops"][0]["args"] = [0, 99]
        with self.assertRaises(IrError):
            seal(payload)

    def test_non_ssa_argument_is_rejected(self) -> None:
        payload = sample_hir()
        payload["ops"][0]["args"] = [1, 0]
        with self.assertRaises(IrError):
            seal(payload)

    def test_deterministic_graph_rejects_neural_effect(self) -> None:
        payload = sample_hir()
        payload["ops"][0]["effects"] = ["neural"]
        with self.assertRaises(IrError):
            seal(payload)

    def test_non_deterministic_graph_accepts_neural_effect(self) -> None:
        payload = sample_hir()
        payload["deterministic"] = False
        payload["ops"][0]["effects"] = ["neural"]
        sealed = seal(payload)
        verify(sealed)

    def test_roundtrip_bytes(self) -> None:
        sealed = seal(sample_hir())
        blob = to_bytes(sealed)
        self.assertEqual(blob, to_bytes(sealed))
        self.assertIn(b'"hash":"blake2b512-128:', blob)
        self.assertEqual(from_bytes(blob), sealed)
        self.assertIsInstance(blob, bytes)

    def test_invalid_json_is_rejected(self) -> None:
        with self.assertRaises(IrError):
            from_bytes(b"{not json")

    def test_output_type_must_match_value(self) -> None:
        payload = sample_hir()
        payload["outputs"][0]["type"] = "i32"
        with self.assertRaises(IrError):
            seal(payload)

    def test_canonical_json_has_no_spaces(self) -> None:
        blob = canonical_bytes(sample_hir())
        self.assertNotIn(b" ", blob)
        json.loads(blob)  # sigue siendo JSON válido


if __name__ == "__main__":
    unittest.main()
