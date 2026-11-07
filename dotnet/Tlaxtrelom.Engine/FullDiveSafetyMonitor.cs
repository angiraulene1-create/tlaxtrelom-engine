// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

using System;

namespace Tlaxtrelom.Engine
{
    public enum SafetyStatus
    {
        Nominal,
        Warning,
        EmergencyDisengagement,
        Detached
    }

    /// <summary>
    /// Monitor de seguridad médica y neuronal para FullDive en C#.
    /// </summary>
    public sealed class FullDiveSafetyMonitor
    {
        public const float MaxSafeCurrentMicroAmps = 50.0f;
        public const float MaxSafeFrequencyHz = 250.0f;
        public const float MaxThermalDeltaCelsius = 2.0f;
        public const uint MaxUnsafeWindowMs = 500;

        private uint _consecutiveUnsafeMs;
        private float _lastMotorRhythmHz;
        private SafetyStatus _status;
        private float _fadeFactor = 1.0f;

        public event Action<string>? OnEmergencyTriggered;

        public SafetyStatus Status => _status;
        public float FadeFactor => _fadeFactor;

        public FullDiveSafetyMonitor()
        {
            _status = SafetyStatus.Nominal;
            _consecutiveUnsafeMs = 0;
            _lastMotorRhythmHz = 0.0f;
        }

        public void UpdateMotorRhythm(float dominantFrequencyHz)
        {
            _lastMotorRhythmHz = dominantFrequencyHz;
        }

        /// <summary>
        /// Evalúa los signos vitales y actualiza el estado de seguridad.
        /// </summary>
        public SafetyStatus InspectVitals(BiometricVitals vitals, uint dtMs)
        {
            if (_status == SafetyStatus.EmergencyDisengagement || _status == SafetyStatus.Detached)
            {
                // Disminución gradual suave (300 ms fade-out)
                _fadeFactor = Math.Max(0.0f, _fadeFactor - (float)dtMs / 300.0f);
                if (_fadeFactor <= 0.0f)
                {
                    _status = SafetyStatus.Detached;
                }
                return _status;
            }

            if (vitals.IsSafe)
            {
                _consecutiveUnsafeMs = 0;
                _status = SafetyStatus.Nominal;
            }
            else
            {
                _consecutiveUnsafeMs += dtMs;
                if (_consecutiveUnsafeMs >= MaxUnsafeWindowMs)
                {
                    _status = SafetyStatus.EmergencyDisengagement;
                    OnEmergencyTriggered?.Invoke($"Telemetría biológica insegura sostenida ({_consecutiveUnsafeMs}ms): HR={vitals.HeartRateBpm}, SpO2={vitals.BloodOxygenSpO2}");
                }
                else
                {
                    _status = SafetyStatus.Warning;
                }
            }
            return _status;
        }

        /// <summary>
        /// Filtra y acota el paquete somatosensorial para proteger los tejidos y evitar
        /// resonancia cortical.
        /// </summary>
        public SomatosensoryPacket SanitizeFeedback(SomatosensoryPacket pkt)
        {
            if (_status == SafetyStatus.EmergencyDisengagement || _status == SafetyStatus.Detached)
            {
                pkt.CurrentMicroAmps = 0.0f;
                pkt.DurationMs = 0;
                pkt.ThermalDeltaCelsius = 0.0f;
                return pkt;
            }

            pkt.CurrentMicroAmps = Math.Clamp(pkt.CurrentMicroAmps, 0.0f, MaxSafeCurrentMicroAmps);
            pkt.FrequencyHz = Math.Clamp(pkt.FrequencyHz, 1.0f, MaxSafeFrequencyHz);
            pkt.DurationMs = Math.Min(pkt.DurationMs, (ushort)50);
            pkt.ThermalDeltaCelsius = Math.Clamp(pkt.ThermalDeltaCelsius, -MaxThermalDeltaCelsius, MaxThermalDeltaCelsius);

            // Detección de resonancia cortical (bloqueo de fase entre estímulo y corteza motora)
            if (_lastMotorRhythmHz > 1.0f && Math.Abs(pkt.FrequencyHz - _lastMotorRhythmHz) < 1.5f)
            {
                // Atenuación anti-bucle inmediata
                pkt.CurrentMicroAmps *= 0.20f;
            }

            return pkt;
        }

        public void TriggerEmergencyStop(string reason)
        {
            _status = SafetyStatus.EmergencyDisengagement;
            OnEmergencyTriggered?.Invoke($"E-Stop manual activado: {reason}");
        }

        public void Reset()
        {
            _status = SafetyStatus.Nominal;
            _consecutiveUnsafeMs = 0;
            _fadeFactor = 1.0f;
        }
    }
}
