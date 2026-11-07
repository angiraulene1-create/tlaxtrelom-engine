// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Métricas de frame y *pacing* determinista.
//!
//! Todo se mide con buffers de tamaño fijo: el registro por frame no asigna y
//! el cálculo de percentiles (una vez por segundo) usa una copia en pila. Es la
//! base para los SLO de `docs/07-estabilidad.md`.

use std::time::{Duration, Instant};

/// Muestras retenidas (5 s a 120 Hz). Suficiente para p95/p99 estables.
const RING: usize = 600;

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameSample {
    /// Tiempo total de CPU del frame (sin presentar) en µs.
    pub frame_us: u32,
    /// Pasos de simulación ejecutados en este frame.
    pub sim_steps: u8,
    /// Se descartó tiempo de simulación (espiral de la muerte evitada).
    pub sim_overrun: bool,
    /// Timeout o pérdida reportada por la superficie (recuperación).
    pub surface_recovered: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MetricsSnapshot {
    pub frames: u64,
    pub errors: u64,
    pub sim_overruns: u64,
    pub surface_recoveries: u64,
    pub p50_us: u32,
    pub p95_us: u32,
    pub p99_us: u32,
    pub max_us: u32,
    pub avg_us: u32,
    pub fps: f32,
}

impl std::fmt::Display for MetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "frames={} fps={:.1} p50={}us p95={}us p99={}us max={}us \
             overruns={} surface={} errors={}",
            self.frames,
            self.fps,
            self.p50_us,
            self.p95_us,
            self.p99_us,
            self.max_us,
            self.sim_overruns,
            self.surface_recoveries,
            self.errors
        )
    }
}

/// Acumulador sin asignaciones. `record` es O(1); `snapshot` es O(n log n) y se
/// llama fuera del camino caliente.
#[derive(Debug)]
pub struct FrameMetrics {
    samples: [u32; RING],
    cursor: usize,
    len: usize,
    frames: u64,
    errors: u64,
    sim_overruns: u64,
    surface_recoveries: u64,
    total_us: u64,
    max_us: u32,
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self {
            samples: [0; RING],
            cursor: 0,
            len: 0,
            frames: 0,
            errors: 0,
            sim_overruns: 0,
            surface_recoveries: 0,
            total_us: 0,
            max_us: 0,
        }
    }
}

impl FrameMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn record(&mut self, sample: FrameSample) {
        let us = sample.frame_us;
        self.samples[self.cursor] = us;
        self.cursor = (self.cursor + 1) % RING;
        self.len = (self.len + 1).min(RING);
        self.frames = self.frames.saturating_add(1);
        self.total_us = self.total_us.saturating_add(us as u64);
        self.max_us = self.max_us.max(us);
        self.sim_overruns += sample.sim_overrun as u64;
        self.surface_recoveries += sample.surface_recovered as u64;
    }

    pub fn record_error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        if self.len == 0 {
            return MetricsSnapshot {
                frames: self.frames,
                errors: self.errors,
                sim_overruns: self.sim_overruns,
                surface_recoveries: self.surface_recoveries,
                ..MetricsSnapshot::default()
            };
        }

        let mut sorted = [0u32; RING];
        sorted[..self.len].copy_from_slice(&self.samples[..self.len]);
        let slice = &mut sorted[..self.len];
        slice.sort_unstable();

        let avg_us = (self.total_us / self.frames.max(1)) as u32;
        let fps = if avg_us > 0 {
            1_000_000.0 / avg_us as f32
        } else {
            0.0
        };

        MetricsSnapshot {
            frames: self.frames,
            errors: self.errors,
            sim_overruns: self.sim_overruns,
            surface_recoveries: self.surface_recoveries,
            p50_us: percentile(slice, 0.50),
            p95_us: percentile(slice, 0.95),
            p99_us: percentile(slice, 0.99),
            max_us: self.max_us,
            avg_us,
            fps,
        }
    }
}

fn percentile(sorted: &[u32], q: f32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    // Nearest-rank: el percentil p es el valor en la posición ceil(p·n),
    // 1-based. Para n=100, p50 devuelve el 50.º valor y p99 el 99.º.
    let q = q.clamp(0.0, 1.0);
    let rank = (q * sorted.len() as f32).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// Pacing de frames con `WaitUntil`: evita el `Poll` que quema un core.
///
/// Si el frame anterior se pasó del deadline, se resincroniza al siguiente
/// periodo en lugar de acumular deuda (sin "spiral of death").
#[derive(Debug, Clone)]
pub struct FramePacer {
    period: Duration,
    next: Instant,
}

impl FramePacer {
    pub fn new(fps: u32, now: Instant) -> Self {
        let fps = fps.clamp(1, 480);
        Self {
            period: Duration::from_secs_f64(1.0 / fps as f64),
            next: now,
        }
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    pub fn next_deadline(&self) -> Instant {
        self.next
    }

    /// Debe llamarse exactamente una vez por frame presentado.
    pub fn on_frame(&mut self, now: Instant) {
        let ideal = self.next + self.period;
        // Si llegamos tarde más de un periodo, no intentamos recuperar todos
        // los frames perdidos: el usuario ya vio el hueco.
        self.next = if ideal < now {
            now + self.period
        } else {
            ideal
        };
    }

    /// Fuerza el próximo deadline (p. ej. tras un resize o suspensión).
    pub fn reset(&mut self, now: Instant) {
        self.next = now + self.period;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_zeroed() {
        let s = FrameMetrics::new().snapshot();
        assert_eq!(s.frames, 0);
        assert_eq!(s.p99_us, 0);
    }

    #[test]
    fn percentiles_are_ordered() {
        let mut m = FrameMetrics::new();
        for i in 1..=100 {
            m.record(FrameSample {
                frame_us: i,
                ..FrameSample::default()
            });
        }
        let s = m.snapshot();
        assert_eq!(s.p50_us, 50);
        assert!(s.p95_us >= s.p50_us);
        assert!(s.p99_us >= s.p95_us);
        assert_eq!(s.max_us, 100);
    }

    #[test]
    fn ring_buffer_accepts_more_than_capacity() {
        let mut m = FrameMetrics::new();
        for _ in 0..(RING * 3) {
            m.record(FrameSample {
                frame_us: 8_000,
                sim_overrun: true,
                ..FrameSample::default()
            });
        }
        let s = m.snapshot();
        assert_eq!(s.frames, (RING * 3) as u64);
        assert_eq!(s.p50_us, 8_000);
        assert_eq!(s.sim_overruns, (RING * 3) as u64);
    }

    #[test]
    fn pacer_resyncs_when_late() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(120, t0);
        p.on_frame(t0 + Duration::from_millis(50));
        let next = p.next_deadline();
        assert!(next >= t0 + Duration::from_millis(50));
        assert!(next <= t0 + Duration::from_millis(60));
    }

    #[test]
    fn pacer_keeps_cadence_when_on_time() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(120, t0);
        p.on_frame(t0);
        let first = p.next_deadline();
        p.on_frame(first);
        assert!(p.next_deadline() > first);
    }
}
