# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""Tlaxtrelom tools — pipeline, autoría y bindings del editor.

Este paquete **no** forma parte del runtime de shipping. Se importa desde el
editor, la CLI de cocina y los tests.
"""

from .graph import Graph, Port, T, Value, node
from .blocks import Script
from .bindings import (
    Engine,
    NativeUnavailable,
    compile_graph,
    compile_hir,
)
from .ir import IrError, hash_hir, seal, verify

__version__ = "0.2.0"

__all__ = [
    "Graph",
    "Port",
    "T",
    "Value",
    "node",
    "Script",
    "Engine",
    "NativeUnavailable",
    "compile_graph",
    "compile_hir",
    "IrError",
    "hash_hir",
    "seal",
    "verify",
    "__version__",
]
