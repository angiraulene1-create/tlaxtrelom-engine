// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

using System;
using System.Runtime.InteropServices;

namespace Tlaxtrelom.Engine.Tests
{
    public static class TlaxtrelomTests
    {
        public static int RunAll()
        {
            int passed = 0;
            Console.WriteLine("[C# Tests] Iniciando suite de pruebas de Tlaxtrelom.Engine...");

            TestStructSizes(ref passed);
            TestOneEuroFilter(ref passed);
            TestSafetyMonitor(ref passed);
            TestSpatialAudio(ref passed);

            Console.WriteLine($"[C# Tests] Éxito: {passed} pruebas pasaron correctamente.");
            return 0;
        }

        private static void TestStructSizes(ref int passed)
        {
            int intentSize = Marshal.SizeOf<IntentFrame>();
            if (intentSize != 56)
            {
                throw new InvalidOperationException($"IntentFrame size mismatch: expected 56, got {intentSize}");
            }

            var frame = IntentFrame.Neutral();
            if (frame.IsValid)
            {
                throw new InvalidOperationException("Neutral frame should not be valid");
            }
            passed++;
            Console.WriteLine("  ✓ IntentFrame tamaño 56B verificado.");
        }

        private static void TestOneEuroFilter(ref int passed)
        {
            var filter = new OneEuroFilter(minCutoff: 1.0f, beta: 0.0f);
            float dt = 0.011f;

            // Inicialización
            filter.Filter(10.0f, dt);

            float last = 0.0f;
            for (int i = 0; i < 10; i++)
            {
                float noise = (i % 2 == 0) ? 10.05f : 9.95f;
                last = filter.Filter(noise, dt);
            }

            if (Math.Abs(last - 10.0f) > 0.03f)
            {
                throw new InvalidOperationException($"OneEuroFilter failed to smooth jitter: {last}");
            }
            passed++;
            Console.WriteLine("  ✓ OneEuroFilter reducción de jitter verificada.");
        }

        private static void TestSafetyMonitor(ref int passed)
        {
            var monitor = new FullDiveSafetyMonitor();
            var safe = BiometricVitals.Default;
            if (monitor.InspectVitals(safe, 100) != SafetyStatus.Nominal)
            {
                throw new InvalidOperationException("Vitals should be nominal");
            }

            // Simular taquicardia sostenida > 500 ms
            var dangerous = new BiometricVitals { HeartRateBpm = 200.0f, BloodOxygenSpO2 = 88.0f };
            monitor.InspectVitals(dangerous, 200); // 200 ms -> Warning
            if (monitor.Status != SafetyStatus.Warning)
            {
                throw new InvalidOperationException("Should be warning");
            }

            monitor.InspectVitals(dangerous, 350); // 550 ms -> Emergency
            if (monitor.Status != SafetyStatus.EmergencyDisengagement)
            {
                throw new InvalidOperationException("Should trigger EmergencyDisengagement");
            }

            // Somatosensory clamping
            monitor.Reset();
            monitor.UpdateMotorRhythm(22.0f);
            var pkt = SomatosensoryPacket.Create(1, 22.1f, 100.0f, 80, 5.0f);
            var sanitized = monitor.SanitizeFeedback(pkt);

            if (sanitized.DurationMs > 50)
            {
                throw new InvalidOperationException("Duration was not clamped");
            }
            if (sanitized.ThermalDeltaCelsius > 2.0f)
            {
                throw new InvalidOperationException("Thermal delta was not clamped");
            }
            if (sanitized.CurrentMicroAmps > 10.1f) // 50 * 0.2 = 10 µA debido a resonancia con 22 Hz
            {
                throw new InvalidOperationException($"Resonance clamping failed: {sanitized.CurrentMicroAmps}");
            }
            passed++;
            Console.WriteLine("  ✓ FullDiveSafetyMonitor y limitador somatosensorial verificados.");
        }

        private static void TestSpatialAudio(ref int passed)
        {
            var gains = SpatialAudio.CalculateBinauralGains(0, 0, 0, 5.0f, 0.0f);
            if (gains.RightGain <= gains.LeftGain)
            {
                throw new InvalidOperationException("Sound to the right should have greater right gain");
            }
            passed++;
            Console.WriteLine("  ✓ SpatialAudio ITD/ILD verificado.");
        }
    }
}
