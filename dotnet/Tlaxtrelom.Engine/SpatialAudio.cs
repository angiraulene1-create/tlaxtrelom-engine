// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

using System;

namespace Tlaxtrelom.Engine
{
    /// <summary>
    /// Procesador de audio espacial VR: calcula ganancias estéreo interaurales
    /// (ITD / ILD - Interaural Time & Level Difference).
    /// </summary>
    public static class SpatialAudio
    {
        public struct BinauralGains
        {
            public float LeftGain;
            public float RightGain;
            public float DelayMs;
        }

        public static BinauralGains CalculateBinauralGains(
            float listenerX, float listenerZ, float listenerYaw,
            float sourceX, float sourceZ)
        {
            float dx = sourceX - listenerX;
            float dz = sourceZ - listenerZ;
            float dist = MathF.Sqrt(dx * dx + dz * dz);

            if (dist < 1e-4f)
            {
                return new BinauralGains { LeftGain = 1.0f, RightGain = 1.0f, DelayMs = 0.0f };
            }

            // Ángulo hacia la fuente de sonido
            float angle = MathF.Atan2(dx, dz) - listenerYaw;

            // Pan estéreo basado en la sombra de la cabeza y diferencias interaurales
            float sinAngle = MathF.Sin(angle);
            float left = (1.0f - 0.7f * sinAngle) * 0.5f;
            float right = (1.0f + 0.7f * sinAngle) * 0.5f;

            // Atenuación por distancia (1 / d)
            float attenuation = 1.0f / MathF.Max(1.0f, dist);

            // Retardo interaural máximo típico ~0.65 ms
            float delayMs = MathF.Abs(sinAngle) * 0.65f;

            return new BinauralGains
            {
                LeftGain = Math.Clamp(left * attenuation, 0.0f, 1.0f),
                RightGain = Math.Clamp(right * attenuation, 0.0f, 1.0f),
                DelayMs = delayMs,
            };
        }
    }
}
