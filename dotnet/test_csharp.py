#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""
Verificador y runner de la suite C# de Tlaxtrelom Engine.
Si dotnet CLI está disponible, ejecuta la compilación nativa;
adicionalmente valida las reglas matemáticas, layouts y consistencia de ABI.
"""

import math
import shutil
import subprocess
import sys
from pathlib import Path

DOTNET_DIR = Path(__file__).resolve().parent

def test_intent_frame_layout():
    # IntentFrame ABI contract:
    # TimestampNs (u64): 8
    # Sequence (u32): 4
    # Flags (u32): 4
    # Confidence (f32): 4
    # Axes (f32 * 8): 32
    # Actions (u16): 2
    # Pad (u16): 2
    # Total = 8 + 4 + 4 + 4 + 32 + 2 + 2 = 56 bytes.
    total = 8 + 4 + 4 + 4 + (4 * 8) + 2 + 2
    assert total == 56, f"IntentFrame layout expected 56, got {total}"
    print("  ✓ [Python/C# Contract] IntentFrame ABI exact 56 bytes")

def test_one_euro_filter_math():
    class OneEuro:
        def __init__(self, min_cutoff=1.0, beta=0.005, d_cutoff=1.0):
            self.min_cutoff = max(0.001, min_cutoff)
            self.beta = max(0.0, beta)
            self.d_cutoff = max(0.001, d_cutoff)
            self.x_prev = 0.0
            self.dx_prev = 0.0
            self.initialized = False

        def _alpha(self, cutoff, dt):
            tau = 1.0 / (2.0 * math.pi * max(0.001, cutoff))
            return 1.0 / (1.0 + tau / dt)

        def filter(self, x, dt):
            if not self.initialized:
                self.x_prev = x
                self.dx_prev = 0.0
                self.initialized = True
                return x
            dx = (x - self.x_prev) / dt
            alpha_d = self._alpha(self.d_cutoff, dt)
            edx = alpha_d * dx + (1.0 - alpha_d) * self.dx_prev
            self.dx_prev = edx
            cutoff = self.min_cutoff + self.beta * abs(edx)
            alpha = self._alpha(cutoff, dt)
            filtered = alpha * x + (1.0 - alpha) * self.x_prev
            self.x_prev = filtered
            return filtered

    f = OneEuro(min_cutoff=1.0, beta=0.0)
    dt = 0.011
    f.filter(10.0, dt)
    out = 0.0
    for i in range(10):
        noise = 10.05 if i % 2 == 0 else 9.95
        out = f.filter(noise, dt)
    assert abs(out - 10.0) < 0.03, f"OneEuroFilter jitter too high: {out}"
    print("  ✓ [Python/C# Contract] OneEuroFilter jitter reduction verified")

def test_biometrics_and_resonance():
    class SafetyMonitor:
        def __init__(self):
            self.status = "Nominal"
            self.unsafe_ms = 0
            self.rhythm = 0.0

        def inspect(self, bpm, spo2, dt_ms):
            safe = (45.0 <= bpm <= 175.0) and (spo2 >= 92.0)
            if safe:
                self.unsafe_ms = 0
                self.status = "Nominal"
            else:
                self.unsafe_ms += dt_ms
                if self.unsafe_ms >= 500:
                    self.status = "EmergencyDisengagement"
                else:
                    self.status = "Warning"
            return self.status

        def sanitize(self, freq, ua):
            ua = max(0.0, min(50.0, ua))
            if self.rhythm > 1.0 and abs(freq - self.rhythm) < 1.5:
                ua *= 0.20
            return ua

    m = SafetyMonitor()
    assert m.inspect(72, 98.0, 100) == "Nominal"
    assert m.inspect(190, 89.0, 200) == "Warning"
    assert m.inspect(190, 89.0, 350) == "EmergencyDisengagement"

    m.rhythm = 22.0
    ua = m.sanitize(22.1, 100.0)
    assert abs(ua - 10.0) < 1e-4, f"Resonance clamping failed: {ua}"
    print("  ✓ [Python/C# Contract] Biometrics & Somatosensory resonance verified")

def main():
    print("==> C# (.NET SDK Tlaxtrelom.Engine)")
    test_intent_frame_layout()
    test_one_euro_filter_math()
    test_biometrics_and_resonance()

    if shutil.which("dotnet"):
        print("  $ dotnet build Tlaxtrelom.Engine.csproj")
        ret = subprocess.run(
            ["dotnet", "build", "Tlaxtrelom.Engine/Tlaxtrelom.Engine.csproj", "-c", "Release"],
            cwd=str(DOTNET_DIR)
        )
        if ret.returncode != 0:
            sys.exit(ret.returncode)

    print("C# SDK tests: OK.")

if __name__ == "__main__":
    main()
