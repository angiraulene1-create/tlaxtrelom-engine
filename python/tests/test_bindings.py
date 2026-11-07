# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Tests del puente Python ↔ Rust y de la CLI."""

from __future__ import annotations

import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from tlaxtrelom_tools import NativeUnavailable
from tlaxtrelom_tools.bindings import Engine, compile_hir, demo
from tlaxtrelom_tools.cli import demo_graph, main
from tlaxtrelom_tools.ir import seal


class BindingsTests(unittest.TestCase):
    def test_native_compile_reports_actionable_error(self) -> None:
        # En CI no se compila PyO3: el mensaje debe decir cómo habilitarlo.
        with self.assertRaises(NativeUnavailable) as ctx:
            compile_hir({"schema": "tlaxtrelom.ir/1"})
        self.assertIn("_native", str(ctx.exception))

    def test_engine_requires_native_module(self) -> None:
        with self.assertRaises(NativeUnavailable):
            Engine()

    def test_demo_produces_verifiable_hir(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            hir = demo()
        self.assertTrue(hir["hash"].startswith("blake2b512-128:"))
        self.assertEqual(hir["module"], "player_locomotion")


class CliTests(unittest.TestCase):
    def _run(self, argv: list[str]) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_selftest_json(self) -> None:
        code, out, err = self._run(["selftest", "--json"])
        self.assertEqual(code, 0, err)
        report = json.loads(out)
        self.assertTrue(report["ok"])
        self.assertIn("determinism_gate", report["checks"])

    def test_check_and_hash_file(self) -> None:
        hir = seal(demo_graph())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "hir.json"
            path.write_text(json.dumps(hir), encoding="utf-8")

            code, out, _ = self._run(["check", str(path)])
            self.assertEqual(code, 0)
            self.assertTrue(json.loads(out)["ok"])

            code, out, _ = self._run(["hash", str(path)])
            self.assertEqual(code, 0)
            self.assertEqual(out.strip(), hir["hash"])

    def test_check_rejects_tampered_hash(self) -> None:
        hir = seal(demo_graph())
        hir["module"] = "tampered"
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "hir.json"
            path.write_text(json.dumps(hir), encoding="utf-8")
            code, _, err = self._run(["check", str(path)])
        self.assertEqual(code, 2)
        self.assertIn("hash inválido", err)

    def test_invalid_json_returns_usage_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "broken.json"
            path.write_text("{", encoding="utf-8")
            code, _, err = self._run(["check", str(path)])
        self.assertEqual(code, 2)
        self.assertIn("error:", err)


if __name__ == "__main__":
    unittest.main()
