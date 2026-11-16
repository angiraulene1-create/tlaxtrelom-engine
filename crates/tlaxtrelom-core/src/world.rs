// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Mundo de demostración y contrato de simulación determinista.
//!
//! El estado autoritativo avanza a paso fijo; el render interpola entre el
//! estado anterior y el actual. Esto desacopla la tasa de simulación de la de
//! presentación, requisito tanto para 120 Hz de escritorio como para 90 Hz de
//! VR con reproyección.
//!
//! Las coordenadas son de dos capas (ver [`crate::coords`]): el jugador vive en
//! un offset `f32` siempre cercano al origen de render, y su posición global se
//! acumula en un anclaje `f64`. No hay un límite de arena: al caminar lejos, el
//! offset se pliega al anclaje y el `f32` recupera precisión completa.

use crate::coords::{self, LocalPose, WorldOrigin};
use crate::fulldive::{IntentFrame, NeuroEvent};
use crate::xr::HeadPose;

/// Altura de ojos por defecto (metros). En VR la pose real la sustituye.
pub const EYE_HEIGHT: f32 = 1.70;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Transform {
    /// Posición **relativa al origen de render** (offset `f32`), nunca global.
    pub position: [f32; 3],
    pub yaw: f32,
}

pub struct World {
    /// Origen flotante: dónde está el `(0,0,0)` local en coordenadas globales.
    pub origin: WorldOrigin,
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
            origin: WorldOrigin::ZERO,
            player,
            previous: player,
            render: player,
            velocity: [0.0; 3],
            last_event: None,
            actions: 0,
            steps: 0,
        }
    }

    /// Demo que arranca lejos del cero global: es el escenario donde las
    /// coordenadas absolutas `f32` perdían el estéreo.
    pub fn demo_at(anchor: [f64; 3]) -> Self {
        let mut w = Self::demo();
        w.origin = WorldOrigin::new(anchor);
        w
    }

    pub fn reset(&mut self) {
        *self = Self::demo();
    }

    /// Posición global del jugador (anclaje + offset), en `f64`.
    pub fn global_position(&self) -> [f64; 3] {
        [
            self.origin.anchor[0] + f64::from(self.player.position[0]),
            self.origin.anchor[1] + f64::from(self.player.position[1]),
            self.origin.anchor[2] + f64::from(self.player.position[2]),
        ]
    }

    /// Paso autoritativo. `dt` es constante (1 / sim_hz), lo que hace que dos
    /// ejecuciones con el mismo input produzcan el mismo estado.
    pub fn fixed_update(&mut self, dt: f32, intent: &IntentFrame) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        // El estado propio se sana antes de integrar: un NaN que entró por otra
        // vía (guardado/carga, bug previo) se neutraliza aquí en vez de
        // propagarse al anclaje `f64`, donde ya no hay forma de recuperarlo.
        self.sanitize_state();
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

        self.player.position[0] += self.velocity[0] * dt;
        self.player.position[1] = EYE_HEIGHT;
        self.player.position[2] += self.velocity[2] * dt;
        self.player.yaw = wrap_angle(self.player.yaw + axes[2] * 1.8 * dt);
        self.actions = intent.actions;

        self.rebase_if_needed();

        debug_assert!(self.player.position.iter().all(|v| v.is_finite()));
    }

    /// Pliega el offset al anclaje cuando el jugador se aleja del origen.
    ///
    /// Los tres estados locales (`player`, `previous`, `render`) se mueven
    /// juntos: plegar solo el autoritativo produciría un salto de hasta
    /// `REBASE_THRESHOLD` metros en el frame interpolado.
    fn rebase_if_needed(&mut self) {
        let pose = LocalPose {
            offset: self.player.position,
            yaw: self.player.yaw,
        };
        if !pose.should_rebase() {
            return;
        }
        let (origin, rebased) = coords::rebase(self.origin, pose);
        let folded = [
            self.player.position[0] - rebased.offset[0],
            self.player.position[1] - rebased.offset[1],
            self.player.position[2] - rebased.offset[2],
        ];
        self.origin = origin;
        self.player.position = rebased.offset;
        self.player.yaw = rebased.yaw;
        for state in [&mut self.previous, &mut self.render] {
            for (axis, fold) in state.position.iter_mut().zip(folded) {
                *axis -= fold;
            }
        }
    }

    /// Devuelve el estado propio a valores finitos. El anclaje no se toca: si
    /// llegara corrupto, ya no representa el mundo.
    fn sanitize_state(&mut self) {
        if !self.player.position.iter().all(|v| v.is_finite()) {
            self.player.position = [0.0, EYE_HEIGHT, 0.0];
        }
        if !self.player.yaw.is_finite() {
            self.player.yaw = 0.0;
        }
        if !self.velocity.iter().all(|v| v.is_finite()) {
            self.velocity = [0.0; 3];
        }
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
    ///
    /// Incluye el anclaje `f64`: dos runs que rebasan de la misma manera tienen
    /// el mismo par `(origen, offset)`, y uno que no rebase debe distinguirse.
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for a in self.origin.anchor.iter() {
            h = fnv_u64(h, a.to_bits());
        }
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
    fn walking_far_rebases_instead_of_clamping() {
        // Sin límite de arena: 1000 s a 6 m/s son 6 km, y el offset sigue
        // cerca del origen mientras el anclaje absorbe la distancia.
        let mut w = World::demo();
        let dt = 1.0 / 120.0;
        for _ in 0..120_000 {
            w.fixed_update(dt, &intent(1.0, 1.0, 0.0));
        }
        assert!(
            w.player.position[0].abs() < coords::REBASE_THRESHOLD + 1.0,
            "offset sin rebasar: {:?}",
            w.player.position
        );
        assert!(
            w.origin.anchor[0] > 5_000.0,
            "anclaje: {:?}",
            w.origin.anchor
        );
        assert!(w.global_position()[0] > 5_000.0);
    }

    #[test]
    fn rebase_keeps_global_position_continuous() {
        // El offset `f32` sí salta al plegarse (ese es el objetivo), pero la
        // posición global y el estado interpolado no pueden moverse más de un
        // paso: plegar `player` sin plegar `previous`/`render` cortaría 1 km.
        let mut w = World::demo();
        let dt = 1.0 / 120.0;
        let mut last_global = w.global_position();
        let mut last_anchor = w.origin.anchor;
        let mut folds = 0u32;
        for _ in 0..60_000 {
            w.fixed_update(dt, &intent(1.0, 0.0, 0.0));
            let global = w.global_position();
            let step = (global[0] - last_global[0]).abs();
            assert!(step < 0.1, "la posición global saltó {step:.1} m");
            last_global = global;
            if w.origin.anchor != last_anchor {
                folds += 1;
                last_anchor = w.origin.anchor;
            }
            w.interpolate(0.5);
            let render_global = w.origin.anchor[0] + f64::from(w.render.position[0]);
            let drift = (render_global - global[0]).abs();
            assert!(
                drift < 0.1,
                "el render quedó a {drift:.1} m del autoritativo"
            );
        }
        assert!(folds >= 2, "el recorrido nunca rebasó el umbral");
    }

    #[test]
    fn corrupt_state_is_neutralized_before_it_reaches_the_anchor() {
        let mut w = World::demo();
        w.player.position[0] = f32::NAN;
        w.velocity[2] = f32::INFINITY;
        w.fixed_update(1.0 / 120.0, &intent(0.0, 0.0, 0.0));
        assert!(w.player.position.iter().all(|v| v.is_finite()));
        assert!(w.velocity.iter().all(|v| v.is_finite()));
        assert!(w.origin.anchor.iter().all(|a| a.is_finite()));
        assert_eq!(w.origin.anchor, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn digest_is_same_at_far_anchor_after_rebase() {
        // Escala: arrancar a 1e11 m (≈6.7 UA) del cero global y caminar. El
        // offset debe quedar finito y pequeño; dos runs iguales, mismo hash.
        fn run(anchor: [f64; 3]) -> (World, u64) {
            let mut w = World::demo_at(anchor);
            let input = intent(0.9, 0.9, 0.05);
            for _ in 0..20_000 {
                w.fixed_update(1.0 / 120.0, &input);
            }
            let h = w.digest();
            (w, h)
        }
        let far = [1e11, 0.0, -4e10];
        let (a, ha) = run(far);
        let (b, hb) = run(far);
        assert_eq!(ha, hb);
        assert_eq!(a.origin, b.origin);
        assert!(a.player.position.iter().all(|v| v.is_finite()));
        assert!(a.player.position[0].abs() < coords::REBASE_THRESHOLD);
        assert!(a.global_position()[0] > 1e11);
        // El mismo recorrido pegado al origen da otro hash: el anclaje importa.
        let (_, h_home) = run([0.0, 0.0, 0.0]);
        assert_ne!(ha, h_home);
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
