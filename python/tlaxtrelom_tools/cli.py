# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""CLI de herramientas de autoría de Tlaxtrelom Engine.

Comandos pensados para el editor, el pipeline de assets y CI:

    python -m tlaxtrelom_tools.cli selftest [--json]
    python -m tlaxtrelom_tools.cli demo [--json]
    python -m tlaxtrelom_tools.cli check <hir.json|-> [--json]
    python -m tlaxtrelom_tools.cli hash <hir.json|->

No enlaza el runtime de shipping: solo Python puro y, si está compilado, el
módulo opcional PyO3.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Sequence

from . import __version__
from .blocks import Script
from .graph import Graph, T, add, neural_read, select, vec3_compose, vec3_scale
from .ir import HASH_PREFIX, IR_SCHEMA, IrError, hash_hir, to_bytes, verify

ENGINE_NAME = "tlaxtrelom-engine"


def demo_graph() -> dict[str, Any]:
    """Grafo de referencia: locomoción determinista."""
    with Graph("player_locomotion") as graph:
        speed = graph.input("speed", T.F32)
        forward = graph.call(
            vec3_compose, x=graph.const(0.0), y=graph.const(0.0), z=graph.const(1.0)
        )
        velocity = graph.call(vec3_scale, v=forward, s=speed)
        graph.output("velocity", velocity)
    return graph.lower()


def demo_script() -> dict[str, Any]:
    """Script de referencia con secuencia y bucle desenrollado."""
    script = Script("turret")
    script.emit(1)
    script.repeat(3, lambda: script.emit(2))
    return script.finish()


def selftest() -> dict[str, Any]:
    """Valida el pipeline Python de punta a punta, sin runtime nativo."""
    checks: list[str] = []

    hir = demo_graph()
    verify(hir)
    checks.append("graph_lower")

    if not hir["hash"].startswith(HASH_PREFIX):
        raise IrError("prefijo de hash inesperado")
    checks.append("hash_prefix")

    again = demo_graph()
    if again["hash"] != hir["hash"]:
        raise IrError("el lowering no es determinista")
    checks.append("graph_deterministic")

    blob = to_bytes(hir)
    if not blob.startswith(b"{"):
        raise IrError("la serialización canónica no es JSON")
    checks.append("canonical_bytes")

    script = demo_script()
    verify(script)
    checks.append("blocks_lower")

    # Un grafo determinista debe rechazar efectos neuronales.
    try:
        with Graph("illegal_neural") as graph:
            channel = graph.const(0)
            value = graph.call(neural_read, channel=channel)
            graph.output("value", value)
        graph.lower()
    except ValueError:
        checks.append("determinism_gate")
    else:
        raise IrError("un grafo determinista aceptó neural.read")

    # Promoción explícita i32 → f32.
    with Graph("promotion") as graph:
        x = graph.input("x", T.I32)
        out = graph.call(add, a=x, b=graph.const(1.0))
        graph.output("out", out)
    promoted = graph.lower()
    if not any(op["op"] == "cast" for op in promoted["ops"]):
        raise IrError("falta el cast explícito i32→f32")
    checks.append("explicit_cast")

    return {
        "engine": ENGINE_NAME,
        "version": __version__,
        "ok": True,
        "schema": IR_SCHEMA,
        "hash": hir["hash"],
        "ops": len(hir["ops"]),
        "script_ops": len(script["ops"]),
        "checks": checks,
    }


def _read_hir(path: str) -> dict[str, Any]:
    raw = sys.stdin.buffer.read() if path == "-" else Path(path).read_bytes()
    payload = json.loads(raw.decode("utf-8"))
    if not isinstance(payload, dict):
        raise IrError("el HIR raíz debe ser un objeto")
    return payload


def _print(payload: dict[str, Any], as_json: bool) -> None:
    if as_json:
        print(json.dumps(payload, indent=2, sort_keys=True))
        return
    if payload.get("ok") is not None:
        state = "OK" if payload["ok"] else "FALLÓ"
        print(f"{ENGINE_NAME} {payload['version']} — selftest {state}")
        for check in payload.get("checks", []):
            print(f"  ✓ {check}")
        print(f"  hash: {payload.get('hash')}")
        print(f"  ops : {payload.get('ops')} (script {payload.get('script_ops')})")
    else:
        print(json.dumps(payload, indent=2, sort_keys=True))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="tlaxtrelom-tools",
        description="Herramientas de autoría de Tlaxtrelom Engine",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    sub = parser.add_subparsers(dest="command", required=True)

    selftest_cmd = sub.add_parser("selftest", help="valida el pipeline Python")
    selftest_cmd.add_argument("--json", action="store_true", help="salida JSON")

    demo_cmd = sub.add_parser("demo", help="emite el HIR de referencia")
    demo_cmd.add_argument("--json", action="store_true", help="salida JSON")

    check_cmd = sub.add_parser("check", help="valida un HIR y su hash")
    check_cmd.add_argument("path", help="archivo JSON o - para stdin")
    check_cmd.add_argument("--json", action="store_true", help="salida JSON")

    hash_cmd = sub.add_parser("hash", help="calcula el hash de un HIR")
    hash_cmd.add_argument("path", help="archivo JSON o - para stdin")

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    try:
        if args.command == "selftest":
            report = selftest()
            _print(report, args.json)
            return 0

        if args.command == "demo":
            hir = demo_graph()
            _print(hir, True)
            return 0

        if args.command == "check":
            hir = _read_hir(args.path)
            verify(hir)
            result = {
                "ok": True,
                "engine": ENGINE_NAME,
                "hash": hir["hash"],
                "module": hir.get("module"),
                "ops": len(hir.get("ops", [])),
            }
            _print(result, args.json or True)
            return 0

        if args.command == "hash":
            hir = _read_hir(args.path)
            print(hash_hir(hir))
            return 0
    except (IrError, ValueError, OSError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    print(f"comando desconocido: {args.command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
