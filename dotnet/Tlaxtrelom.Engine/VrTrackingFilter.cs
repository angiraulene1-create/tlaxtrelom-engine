// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

using System;

namespace Tlaxtrelom.Engine
{
    /// <summary>
    /// Filtro 1€ (One-Euro Filter) en C# de alto rendimiento sin asignaciones.
    /// Suprime el temblor en reposo y responde sin retardo a movimientos VR rápidos.
    /// </summary>
    public sealed class OneEuroFilter
    {
        private readonly float _minCutoff;
        private readonly float _beta;
        private readonly float _dCutoff;
        private float _xPrev;
        private float _dxPrev;
        private bool _initialized;

        public OneEuroFilter(float minCutoff = 1.0f, float beta = 0.005f, float dCutoff = 1.0f)
        {
            _minCutoff = Math.Max(0.001f, minCutoff);
            _beta = Math.Max(0.0f, beta);
            _dCutoff = Math.Max(0.001f, dCutoff);
            _initialized = false;
        }

        public float Filter(float x, float dtSeconds)
        {
            if (float.IsNaN(x) || float.IsInfinity(x) || dtSeconds <= 0.0f)
            {
                return _xPrev;
            }

            if (!_initialized)
            {
                _xPrev = x;
                _dxPrev = 0.0f;
                _initialized = true;
                return x;
            }

            float dx = (x - _xPrev) / dtSeconds;
            float edx = FilterAlpha(dx, _dxPrev, dtSeconds, _dCutoff);
            _dxPrev = edx;

            float cutoff = _minCutoff + _beta * Math.Abs(edx);
            float filtered = FilterAlpha(x, _xPrev, dtSeconds, cutoff);
            _xPrev = filtered;
            return filtered;
        }

        private static float FilterAlpha(float val, float prev, float dtSeconds, float cutoff)
        {
            float tau = 1.0f / (2.0f * (float)Math.PI * Math.Max(0.001f, cutoff));
            float alpha = 1.0f / (1.0f + tau / dtSeconds);
            return alpha * val + (1.0f - alpha) * prev;
        }

        public void Reset()
        {
            _initialized = false;
            _xPrev = 0.0f;
            _dxPrev = 0.0f;
        }
    }

    /// <summary>
    /// Filtro de seguimiento para 6DOF HeadPose.
    /// </summary>
    public sealed class HeadPoseFilter
    {
        private readonly OneEuroFilter _filterX;
        private readonly OneEuroFilter _filterY;
        private readonly OneEuroFilter _filterZ;
        private readonly OneEuroFilter _filterYaw;

        public HeadPoseFilter(float minCutoff = 1.0f, float beta = 0.005f)
        {
            _filterX = new OneEuroFilter(minCutoff, beta);
            _filterY = new OneEuroFilter(minCutoff, beta);
            _filterZ = new OneEuroFilter(minCutoff, beta);
            _filterYaw = new OneEuroFilter(minCutoff, beta);
        }

        public HeadPose Filter(HeadPose pose, float dtSeconds)
        {
            return new HeadPose(
                _filterX.Filter(pose.X, dtSeconds),
                _filterY.Filter(pose.Y, dtSeconds),
                _filterZ.Filter(pose.Z, dtSeconds),
                _filterYaw.Filter(pose.Yaw, dtSeconds)
            );
        }

        public void Reset()
        {
            _filterX.Reset();
            _filterY.Reset();
            _filterZ.Reset();
            _filterYaw.Reset();
        }
    }
}
