// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

using System;
using System.Runtime.InteropServices;

namespace Tlaxtrelom.Engine
{
    public static class FullDiveFlags
    {
        public const uint Valid = 1u << 0;
        public const uint Calibrating = 1u << 1;
        public const uint Artifact = 1u << 2;
        public const uint Gated = 1u << 3;
        public const uint Action = 1u << 4;
        public const uint Predicted = 1u << 5;
    }

    /// <summary>
    /// Exact representation of IntentFrame (56 bytes, cache-line friendly).
    /// </summary>
    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    public unsafe struct IntentFrame
    {
        public ulong TimestampNs;
        public uint Sequence;
        public uint Flags;
        public float Confidence;
        public fixed float Axes[8];
        public ushort Actions;
        public ushort Pad;

        public bool IsValid => (Flags & FullDiveFlags.Valid) != 0;
        public bool IsGated => (Flags & FullDiveFlags.Gated) != 0;
        public bool HasArtifact => (Flags & FullDiveFlags.Artifact) != 0;

        public static IntentFrame Neutral()
        {
            var frame = new IntentFrame();
            frame.TimestampNs = 0;
            frame.Sequence = 0;
            frame.Flags = 0;
            frame.Confidence = 0.0f;
            frame.Actions = 0;
            frame.Pad = 0;
            for (int i = 0; i < 8; i++)
            {
                frame.Axes[i] = 0.0f;
            }
            return frame;
        }
    }

    /// <summary>
    /// Telemetría biométrica de seguridad para FullDive.
    /// </summary>
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    public struct BiometricVitals
    {
        public float HeartRateBpm;
        public float BloodOxygenSpO2;
        public float GalvanicSkinMicroSiemens;
        public float CoreTempCelsius;
        public float StressIndex;

        public static BiometricVitals Default => new BiometricVitals
        {
            HeartRateBpm = 72.0f,
            BloodOxygenSpO2 = 98.5f,
            GalvanicSkinMicroSiemens = 4.0f,
            CoreTempCelsius = 36.6f,
            StressIndex = 0.15f,
        };

        public bool IsSafe =>
            HeartRateBpm >= 45.0f && HeartRateBpm <= 175.0f &&
            BloodOxygenSpO2 >= 92.0f &&
            CoreTempCelsius >= 35.0f && CoreTempCelsius <= 38.5f &&
            StressIndex <= 0.85f;
    }

    /// <summary>
    /// Paquete de retroalimentación somatosensorial en la corteza/nervios.
    /// </summary>
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    public struct SomatosensoryPacket
    {
        public ushort ChannelId;
        public float FrequencyHz;
        public float CurrentMicroAmps;
        public ushort DurationMs;
        public float ThermalDeltaCelsius;

        public static SomatosensoryPacket Create(ushort channel, float freq, float currentUa, ushort durationMs, float thermalDelta)
        {
            return new SomatosensoryPacket
            {
                ChannelId = channel,
                FrequencyHz = freq,
                CurrentMicroAmps = currentUa,
                DurationMs = durationMs,
                ThermalDeltaCelsius = thermalDelta,
            };
        }
    }

    /// <summary>
    /// Pose 6DOF para headsets VR y controladores hápticos.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct HeadPose
    {
        public float X;
        public float Y;
        public float Z;
        public float Yaw;

        public HeadPose(float x, float y, float z, float yaw)
        {
            X = x;
            Y = y;
            Z = z;
            Yaw = yaw;
        }
    }

    /// <summary>
    /// Información de HAL y arquitectura de plataforma.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct HalInfo
    {
        public uint AbiVersion;
        public uint PageSize;
        public ulong PhysicalMemoryBytes;
        public uint LogicalCores;
        public uint HasAvx2;
        public uint HasAvx512;
        public uint HasNeon;
        public uint HasRealtime;
        public uint SimdLevel;
        public fixed byte Os[32];
        public fixed byte Arch[32];
    }
}
