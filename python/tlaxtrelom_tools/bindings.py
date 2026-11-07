# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Puente Python ↔ Rust (PyO3) y utilidades de compilación.

El runtime nativo expone un módulo `tlaxtrelom._native` compilado con PyO3. Reglas:

- **Nunca** se serializa con `pickle`: el HIR viaja como FlatBuffers (bytes).
- Las llamadas pesadas liberan el GIL (`py.allow_threads`).
- Los arrays de numpy se pasan por *buffer protocol*, sin copia.
- El editor puede usar este módulo; el juego exportado no lo enlaza.

Lado Rust (resumen, `crates/tlaxtrelom-bindings/src/lib.rs`):

    #[pyfunction]
    fn compile_hir(
        py: Python<'_>,
        hir: &[u8],
        target: &str,
        opt_level: u8,
    ) -> PyResult<Py<PyBytes>> {
        let out = py.allow_threads(|| tlaxtrelom_ir::compile(hir, target, opt_level))?;
        Ok(PyBytes::new(py, &out).unbind())
    }

    #[pyfunction]
    fn encode_hir(hir: &Bound<'_, PyAny>) -> PyResult<Py<PyBytes>> { /* dict → FlatBuffers */ }

    #[pymodule]
    fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(compile_hir, m)?)?;
        m.add_class::<Engine>()?;
        Ok(())
    }
"""

from __future__ import annotations

from typing import Any, Iterator

from .graph import Graph

try:
    import numpy as np
except ImportError:  # numpy es opcional para herramientas de solo grafo
    np = None  # type: ignore[assignment]

try:
    from tlaxtrelom import _native  # type: ignore[attr-defined]
except Exception:  # ImportError o binario no compilado
    _native = None


class NativeUnavailable(RuntimeError):
    """El módulo `tlaxtrelom._native` no está disponible en este entorno."""


def _require() -> Any:
    if _native is None:
        raise NativeUnavailable(
            "tlaxtrelom._native no está compilado. Ejecutá `maturin develop` "
            "en crates/tlaxtrelom-bindings o usá el editor empaquetado."
        )
    return _native


# ─────────────────────────────────────────────────────────────────────────────
# Compilación de HIR
# ─────────────────────────────────────────────────────────────────────────────

def compile_hir(
    hir: dict[str, Any],
    *,
    target: str = "spirv",
    opt_level: int = 2,
) -> bytes:
    """Compila un HIR (dict) al backend pedido. Devuelve bytes del artefacto."""
    native = _require()
    blob = native.encode_hir(hir)
    return bytes(native.compile_hir(blob, target=target, opt_level=opt_level))


def compile_graph(
    graph: Graph,
    *,
    target: str = "spirv",
    opt_level: int = 2,
) -> bytes:
    """Atajo: `Graph` → HIR → backend."""
    return compile_hir(graph.lower(), target=target, opt_level=opt_level)


# ─────────────────────────────────────────────────────────────────────────────
# Runtime (editor / tests)
# ─────────────────────────────────────────────────────────────────────────────

class Engine:
    """Handle del motor nativo.

    `push_neural` es zero-copy: el array de numpy se pasa por buffer protocol y
    Rust lo lee sin copiar, liberando el GIL durante el procesamiento.
    """

    def __init__(
        self,
        width: int = 1280,
        height: int = 720,
        *,
        fulldive: bool = True,
    ) -> None:
        native = _require()
        self._handle = native.Engine(int(width), int(height), bool(fulldive))

    def push_neural(
        self,
        samples: Any,
        t0_ns: int,
        sample_rate: int,
    ) -> None:
        if np is None:
            raise NativeUnavailable("numpy es requerido para push_neural")
        arr = np.asarray(samples, dtype=np.float32)
        if arr.ndim != 2:
            raise ValueError(
                f"samples debe ser 2D (muestras, canales); llegó {arr.shape}"
            )
        if not arr.flags.c_contiguous:
            arr = np.ascontiguousarray(arr)
        self._handle.push_neural(arr, int(t0_ns), int(sample_rate))

    def step(self, dt: float) -> None:
        self._handle.step(float(dt))

    def intent(self) -> dict[str, Any]:
        return self._handle.intent()

    def close(self) -> None:
        self._handle.close()

    def __enter__(self) -> "Engine":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> bool:
        self.close()
        return False


# ─────────────────────────────────────────────────────────────────────────────
# Demo sin runtime nativo
# ─────────────────────────────────────────────────────────────────────────────

def demo() -> dict[str, Any]:
    from .graph import T, vec3_compose, vec3_scale

    with Graph("player_locomotion") as graph:
        speed = graph.input("speed", T.F32)
        forward = graph.call(
            vec3_compose, x=graph.const(0.0), y=graph.const(0.0), z=graph.const(1.0)
        )
        velocity = graph.call(vec3_scale, v=forward, s=speed)
        graph.output("velocity", velocity)

    hir = graph.lower()
    print(f"HIR {hir['hash']} — {len(hir['ops'])} ops")
    return hir


if __name__ == "__main__":
    demo()
