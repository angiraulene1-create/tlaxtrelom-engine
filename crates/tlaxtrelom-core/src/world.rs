// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Mundo de demostración y contrato de simulación determinista.
//!
//! El estado autoritativo avanza a paso fijo; el render interpola entre el
//! estado anterior y el actual. Esto desacopla la tasa de simulación de la de
//! presentación, requisito tanto para 120 Hz de escritorio como para 90 Hz de
//! VR con reproyección.

use crate::fulldive::{IntentFrame, NeuroEvent};
use crate::xr::HeadPose;

/// Altura de ojos por defecto (metros). En VR la pose real la sustituye.
pub const EYE_HEIGHT: f32 = 1.70;

/// Límite del área jugable de la demo. Evita que un intent corrupto mande al
/// jugador a `NaN` o a 1e30.
pub const ARENA_LIMIT: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Transform {
    pub position: [f32; 3],
    pub yaw: f32,
}

pub struct World {
    /// Estado autoritativo actual (último paso fijo).
    pub player: Transform,
    /// Estado del paso anterior, usado por `interpolate`.
    pub previous: Transform,
    /// Estado interpolado que consume el render.
    pub render: Transform,
    pub velocity: [f32; 3],
    pub last_event: Option<NeuroEvent>,
    pub actions: u16,
    pub steps: u64,
}

impl World {
    pub fn demo() -> Self {
        let player = Transform {
            position: [0.0, EYE_HEIGHT, 0.0],
            yaw: 0.0,
        };
        Self {
            player,
            previous: player,
            render: player,
            velocity: [0.0; 3],
            last_event: None,
            actions: 0,
            steps: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::demo();
    }

    /// Paso autoritativo. `dt` es constante (1 / sim_hz), lo que hace que dos
    /// ejecuciones con el mismo input produzcan el mismo estado.
    pub fn fixed_update(&mut self, dt: f32, intent: &IntentFrame) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.previous = self.player;
        self.steps = self.steps.saturating_add(1);

        let axes = sanitize_axes(&intent.axes);
        let speed = 6.0; // m/s
        let target = [axes[0] * speed, 0.0, axes[1] * speed];

        // Suavizado de velocidad independiente del gate: aunque el gate ya
        // limita aceleración, el mundo no confía en un input de otro origen.
        let accel = 30.0;
        for (velocity, target) in self.velocity.iter_mut().zip(target) {
            let delta = (target - *velocity).clamp(-accel * dt, accel * dt);
            *velocity = (*velocity + delta).clamp(-speed, speed);
        }

        self.player.position[0] =
            (self.player.position[0] + self.velocity[0] * dt).clamp(-ARENA_LIMIT, ARENA_LIMIT);
        self.player.position[1] = EYE_HEIGHT;
        self.player.position[2] =
            (self.player.position[2] + self.velocity[2] * dt).clamp(-ARENA_LIMIT, ARENA_LIMIT);
        self.player.yaw = wrap_angle(self.player.yaw + axes[2] * 1.8 * dt);
        self.actions = intent.actions;

        debug_assert!(self.player.position.iter().all(|v| v.is_finite()));
    }

    /// Interpolación para el render. `alpha` ∈ [0, 1] es el remanente del
    /// acumulador de simulación dividido por el paso.
    pub fn interpolate(&mut self, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        self.render.position = [
            lerp(self.previous.position[0], self.player.position[0], alpha),
            lerp(self.previous.position[1], self.player.position[1], alpha),
            lerp(self.previous.position[2], self.player.position[2], alpha),
        ];
        self.render.yaw = lerp_angle(self.previous.yaw, self.player.yaw, alpha);
    }

    /// Pose que consume el stack XR. La altura de ojos ya está en `position`.
    pub fn head_pose(&self) -> HeadPose {
        HeadPose {
            position: self.render.position,
            yaw: self.render.yaw,
        }
    }

    pub fn on_neural_event(&mut self, ev: NeuroEvent) {
        self.last_event = Some(ev);
    }

    /// Hash FNV-1a del estado autoritativo. Es estable entre ejecuciones y
    /// plataformas porque hashea bits, no `Debug`. Lo usa el selftest para
    /// probar determinismo y el netcode para comparar predicciones.
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for value in self
            .player
            .position
            .iter()
            .chain(std::iter::once(&self.player.yaw))
            .chain(self.velocity.iter())
        {
            h = fnv_u32(h, value.to_bits());
        }
        h = fnv_u64(h, self.actions as u64);
        fnv_u64(h, self.steps)
    }
}

fn fnv_u32(mut h: u64, value: u32) -> u64 {
    for byte in value.to_le_bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn fnv_u64(mut h: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn sanitize_axes(axes: &[f32; 8]) -> [f32; 8] {
    let mut out = [0.0f32; 8];
    for (i, v) in axes.iter().enumerate() {
        out[i] = if v.is_finite() {
            v.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
    out
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn wrap_angle(mut a: f32) -> f32 {
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let delta = wrap_angle(b - a);
    wrap_angle(a + delta * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fulldive::flags;

    fn intent(x: f32, forward: f32, turn: f32) -> IntentFrame {
        let mut f = IntentFrame::neutral();
        f.axes[0] = x;
        f.axes[1] = forward;
        f.axes[2] = turn;
        f.flags = flags::VALID;
        f
    }

    #[test]
    fn fixed_step_is_deterministic() {
        let mut a = World::demo();
        let mut b = World::demo();
        let dt = 1.0 / 120.0;
        let input = intent(0.4, -0.2, 0.1);
        for _ in 0..600 {
            a.fixed_update(dt, &input);
            b.fixed_update(dt, &input);
        }
        assert_eq!(a.player, b.player);
        assert_eq!(a.velocity, b.velocity);
    }

    #[test]
    fn corrupt_input_never_produces_nan() {
        let mut w = World::demo();
        let mut f = IntentFrame::neutral();
        f.axes[0] = f32::NAN;
        f.axes[1] = f32::INFINITY;
        w.fixed_update(1.0 / 120.0, &f);
        assert!(w.player.position.iter().all(|v| v.is_finite()));
        assert!(w.velocity.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn interpolation_is_between_states() {
        let mut w = World::demo();
        w.previous.position[2] = 0.0;
        w.player.position[2] = 10.0;
        w.interpolate(0.5);
        assert!((w.render.position[2] - 5.0).abs() < 1e-6);
        w.interpolate(0.0);
        assert!((w.render.position[2] - 0.0).abs() < 1e-6);
        w.interpolate(1.0);
        assert!((w.render.position[2] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn yaw_interpolation_takes_short_path() {
        assert!((lerp_angle(3.10, -3.10, 0.5) - std::f32::consts::PI).abs() < 0.05);
    }

    #[test]
    fn arena_limit_is_enforced() {
        let mut w = World::demo();
        let dt = 0.1;
        for _ in 0..1_000 {
            w.fixed_update(dt, &intent(1.0, 1.0, 0.0));
        }
        assert!(w.player.position[0] <= ARENA_LIMIT + 1e-3);
        assert!(w.player.position[2] <= ARENA_LIMIT + 1e-3);
    }

    #[test]
    fn digest_is_stable_and_state_sensitive() {
        let mut a = World::demo();
        let mut b = World::demo();
        let input = intent(0.3, 0.2, 0.0);
        for _ in 0..120 {
            a.fixed_update(1.0 / 120.0, &input);
            b.fixed_update(1.0 / 120.0, &input);
        }
        assert_eq!(a.digest(), b.digest());
        let before = a.digest();
        a.fixed_update(1.0 / 120.0, &input);
        assert_ne!(a.digest(), before);
    }
}
