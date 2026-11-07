// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! `IntentGate`: la frontera de seguridad entre el decoder BCI y el gameplay.
//!
//! El decoder puede producir basura (artefacto, electrodo caído, deriva del
//! modelo). El gate es la última línea antes de mover al jugador o accionar un
//! actuador. Aplica, en este orden:
//!
//! 1. **Watchdog**: sin frame nuevo en `watchdog` ⇒ neutral + `GATED`.
//! 2. **Confianza**: por debajo del umbral continuo ⇒ neutral + `GATED`.
//! 3. **Deadzone + histéresis**: evita temblor alrededor del centro.
//! 4. **Límites de velocidad y aceleración**: un clamp duro, no cosmético.
//! 5. **Dwell** para acciones discretas: 150 ms sostenidos por defecto.
//! 6. **Predicción**: la aplica el host, nunca el gate; así se puede desactivar
//!    en acciones críticas y marcar `PREDICTED`.
//!
//! Todas las operaciones son O(1) y sin asignaciones: el gate vive en el frame
//! loop, no en un hilo de herramientas.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::fulldive::{flags, IntentFrame};

/// Número de bits de acción discreta soportados por `IntentFrame.actions`.
pub const MAX_ACTIONS: usize = 16;

#[derive(Debug, Clone)]
pub struct GateConfig {
    /// Confianza mínima para ejes continuos.
    pub min_continuous_confidence: f32,
    /// Confianza mínima para acciones discretas.
    pub min_discrete_confidence: f32,
    /// Tiempo que una acción debe mantenerse para confirmarse.
    pub dwell: Duration,
    /// Sin datos nuevos durante este tiempo ⇒ neutral.
    pub watchdog: Duration,
    /// Velocidad máxima por eje normalizado (0, 1].
    pub max_speed: [f32; 2],
    /// Aceleración máxima por eje (unidades normalizadas/s).
    pub max_accel: [f32; 2],
    /// Zona muerta radial (0, 1).
    pub deadzone: f32,
    /// Margen extra para salir de la zona muerta (anti-chattering).
    pub hysteresis: f32,
    /// `dt` mínimo usado por el limitador de aceleración.
    pub min_dt: Duration,
    /// `dt` máximo usado por el limitador de aceleración.
    pub max_dt: Duration,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            min_continuous_confidence: 0.35,
            min_discrete_confidence: 0.65,
            dwell: Duration::from_millis(150),
            watchdog: Duration::from_millis(50),
            max_speed: [1.0, 1.0],
            max_accel: [4.0, 4.0],
            deadzone: 0.05,
            hysteresis: 0.03,
            min_dt: Duration::from_millis(1),
            max_dt: Duration::from_millis(50),
        }
    }
}

impl GateConfig {
    pub fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.min_continuous_confidence) {
            bail!("min_continuous_confidence fuera de [0,1]");
        }
        if !(0.0..=1.0).contains(&self.min_discrete_confidence) {
            bail!("min_discrete_confidence fuera de [0,1]");
        }
        if self.min_discrete_confidence < self.min_continuous_confidence {
            bail!("la confianza discreta no puede ser menor que la continua");
        }
        if self.dwell.is_zero() || self.dwell > Duration::from_secs(2) {
            bail!("dwell fuera de (0, 2s]");
        }
        if self.watchdog.is_zero() || self.watchdog > Duration::from_secs(1) {
            bail!("watchdog fuera de (0, 1s]");
        }
        for (i, v) in self.max_speed.iter().enumerate() {
            if !(*v > 0.0 && *v <= 1.0) {
                bail!("max_speed[{i}] fuera de (0,1]: {v}");
            }
        }
        for (i, v) in self.max_accel.iter().enumerate() {
            if !(*v > 0.0 && *v <= 100.0) {
                bail!("max_accel[{i}] fuera de (0,100]: {v}");
            }
        }
        if !(0.0..0.5).contains(&self.deadzone) {
            bail!("deadzone fuera de [0, 0.5)");
        }
        if !(0.0..0.5).contains(&self.hysteresis) {
            bail!("hysteresis fuera de [0, 0.5)");
        }
        if self.min_dt.is_zero() || self.max_dt <= self.min_dt {
            bail!("ventana dt inválida");
        }
        Ok(())
    }
}

/// Estado del gate. Es `!Send` por diseño: vive en el hilo de simulación.
#[derive(Debug, Clone)]
pub struct IntentGate {
    cfg: GateConfig,
    initialized: bool,
    last_rx: Instant,
    last_seq: u32,
    last_valid_at: Option<Instant>,
    speed: [f32; 2],
    prev_axes: [f32; 4],
    /// Estado de histéresis por eje (0..3). Evita chattering en el borde de la
    /// zona muerta sin introducir un filtro que añada latencia.
    engaged: [bool; 4],
    dwell_start: [Option<Instant>; MAX_ACTIONS],
    gated_frames: u64,
    total_frames: u64,
    e_stop: bool,
}

impl IntentGate {
    pub fn new(cfg: GateConfig) -> Self {
        Self {
            cfg,
            initialized: false,
            last_rx: Instant::now(),
            last_seq: 0,
            last_valid_at: None,
            speed: [0.0; 2],
            prev_axes: [0.0; 4],
            engaged: [false; 4],
            dwell_start: [None; MAX_ACTIONS],
            gated_frames: 0,
            total_frames: 0,
            e_stop: false,
        }
    }

    pub fn config(&self) -> &GateConfig {
        &self.cfg
    }

    /// Fuerza neutral. Se llama al perder foco, al abrir un menú o al cambiar de
    /// sesión; evita que la última intención quede "pegada".
    pub fn reset(&mut self) {
        self.speed = [0.0; 2];
        self.prev_axes = [0.0; 4];
        self.engaged = [false; 4];
        self.dwell_start = [None; MAX_ACTIONS];
        self.last_valid_at = None;
        self.initialized = false;
    }

    /// Activa la parada de emergencia física/lógica (E-Stop).
    pub fn trigger_e_stop(&mut self) {
        self.e_stop = true;
        self.reset();
    }

    /// Restablece el gate tras resolver la emergencia.
    pub fn clear_e_stop(&mut self) {
        self.e_stop = false;
        self.reset();
    }

    pub fn is_e_stop(&self) -> bool {
        self.e_stop
    }

    /// Fracción de frames bloqueados desde el último `take_stats`.
    pub fn gated_ratio(&self) -> f32 {
        if self.total_frames == 0 {
            0.0
        } else {
            self.gated_frames as f32 / self.total_frames as f32
        }
    }

    pub fn take_stats(&mut self) -> (u64, u64) {
        let out = (self.gated_frames, self.total_frames);
        self.gated_frames = 0;
        self.total_frames = 0;
        out
    }

    /// Aplica el gate. Devuelve un frame *nuevo*; nunca muta el de entrada.
    pub fn apply(&mut self, raw: IntentFrame, now: Instant) -> IntentFrame {
        self.total_frames = self.total_frames.saturating_add(1);

        let mut out = raw;
        out.flags &= !(flags::VALID | flags::GATED | flags::ACTION);

        if self.e_stop {
            return self.neutral(out, true);
        }

        // Primer frame: arrancamos el reloj del watchdog aquí, no en la creación.
        if !self.initialized {
            self.initialized = true;
            self.last_rx = now;
            self.last_seq = raw.seq;
        }

        let fresh = raw.seq != self.last_seq;
        if fresh {
            self.last_seq = raw.seq;
            self.last_rx = now;
        }

        let watchdog_expired = now.saturating_duration_since(self.last_rx) > self.cfg.watchdog;

        if !fresh && watchdog_expired {
            return self.neutral(out, true);
        }
        if raw.flags & flags::ARTIFACT != 0 || raw.flags & flags::CALIBRATING != 0 {
            return self.neutral(out, true);
        }
        if !raw.is_valid() || raw.confidence < self.cfg.min_continuous_confidence {
            return self.neutral(out, true);
        }

        let dt = self
            .last_valid_at
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or(Duration::from_millis(8))
            .clamp(self.cfg.min_dt, self.cfg.max_dt);
        self.last_valid_at = Some(now);

        // Ejes continuos 0 y 1: deadzone + clamp de velocidad + aceleración.
        let deadzone = self.cfg.deadzone;
        let hysteresis = self.cfg.hysteresis;
        for axis in 0..2 {
            let shaped = deadzone_axis(
                raw.axes[axis],
                deadzone,
                hysteresis,
                &mut self.engaged[axis],
            );
            let target = (shaped * self.cfg.max_speed[axis]).clamp(-1.0, 1.0);
            let max_delta = self.cfg.max_accel[axis] * dt.as_secs_f32();
            let delta = (target - self.speed[axis]).clamp(-max_delta, max_delta);
            self.speed[axis] = (self.speed[axis] + delta).clamp(-1.0, 1.0);
            out.axes[axis] = (self.speed[axis] / self.cfg.max_speed[axis]).clamp(-1.0, 1.0);
        }

        // Ejes 2 y 3: yaw/pitch no llevan aceleración (el usuario espera
        // respuesta inmediata), pero sí deadzone y clamp.
        for axis in 2..4 {
            out.axes[axis] = deadzone_axis(
                raw.axes[axis],
                deadzone,
                hysteresis,
                &mut self.engaged[axis],
            );
        }

        // Acciones discretas con dwell. Si la confianza cae, se reinicia el
        // contador para no confirmar una acción con datos dudosos.
        let mut actions = 0u16;
        for bit in 0..MAX_ACTIONS {
            let pressed = raw.actions & (1u16 << bit) != 0;
            if pressed && raw.confidence >= self.cfg.min_discrete_confidence {
                let since = self.dwell_start[bit].get_or_insert(now);
                if now.saturating_duration_since(*since) >= self.cfg.dwell {
                    actions |= 1u16 << bit;
                }
            } else {
                self.dwell_start[bit] = None;
            }
        }
        out.actions = actions;
        if actions != 0 {
            out.flags |= flags::ACTION;
        }

        // Velocidad estimada para predicción (diferencia finita estable).
        let dt_s = dt.as_secs_f32().max(1e-4);
        for axis in 0..2 {
            let vel = (out.axes[axis] - self.prev_axes[axis]) / dt_s;
            out.axes[axis + 4] = vel.clamp(-1.0, 1.0);
        }
        self.prev_axes = [out.axes[0], out.axes[1], out.axes[2], out.axes[3]];

        out.flags |= flags::VALID;
        out
    }

    fn neutral(&mut self, mut frame: IntentFrame, gated: bool) -> IntentFrame {
        frame.axes = [0.0; 8];
        frame.actions = 0;
        frame.flags &= !flags::VALID;
        if gated {
            frame.flags |= flags::GATED;
            self.gated_frames = self.gated_frames.saturating_add(1);
        }
        self.speed = [0.0; 2];
        self.prev_axes = [0.0; 4];
        self.engaged = [false; 4];
        frame
    }
}

/// Zona muerta con histéresis. La salida es continua para no introducir un
/// escalón que el jugador perciba como un tirón.
///
/// `engaged` es el estado del eje: para **entrar** hace falta superar
/// `dead + hysteresis`; para **salir** basta con caer por debajo de `dead`.
fn deadzone_axis(x: f32, dead: f32, hysteresis: f32, engaged: &mut bool) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    let mag = x.abs();
    let enter = (dead + hysteresis).min(1.0);

    if !*engaged {
        if mag <= enter {
            return 0.0;
        }
        *engaged = true;
    }

    if mag < dead {
        *engaged = false;
        return 0.0;
    }

    let sign = x.signum();
    let scaled = (mag - dead) / (1.0 - dead).max(1e-6);
    sign * scaled.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u32, confidence: f32, x: f32, actions: u16) -> IntentFrame {
        let mut f = IntentFrame::neutral();
        f.seq = seq;
        f.confidence = confidence;
        f.axes[0] = x;
        f.actions = actions;
        f.flags = flags::VALID;
        f
    }

    #[test]
    fn rejects_low_confidence_and_marks_gated() {
        let mut g = IntentGate::new(GateConfig::default());
        let t = Instant::now();
        let out = g.apply(frame(1, 0.1, 1.0, 0), t);
        assert!(!out.is_valid());
        assert_eq!(out.flags & flags::GATED, flags::GATED);
        assert_eq!(out.axes[0], 0.0);
    }

    #[test]
    fn clamps_acceleration() {
        let cfg = GateConfig {
            max_accel: [1.0, 1.0],
            deadzone: 0.0,
            hysteresis: 0.0,
            ..GateConfig::default()
        };
        let mut g = IntentGate::new(cfg);
        let t0 = Instant::now();

        // 10 ms después de un frame válido, no puede haber saltado a 1.0.
        let _ = g.apply(frame(1, 0.9, 0.0, 0), t0);
        let out = g.apply(frame(2, 0.9, 1.0, 0), t0 + Duration::from_millis(10));
        assert!(out.axes[0] > 0.0);
        assert!(out.axes[0] < 0.1, "axis={}", out.axes[0]);
    }

    #[test]
    fn dwell_confirms_action_only_after_time() {
        let cfg = GateConfig {
            dwell: Duration::from_millis(150),
            ..GateConfig::default()
        };
        let mut g = IntentGate::new(cfg);
        let t0 = Instant::now();

        let early = g.apply(frame(1, 0.9, 0.0, 1), t0);
        assert_eq!(early.actions, 0);

        let still_early = g.apply(frame(2, 0.9, 0.0, 1), t0 + Duration::from_millis(100));
        assert_eq!(still_early.actions, 0);

        let confirmed = g.apply(frame(3, 0.9, 0.0, 1), t0 + Duration::from_millis(160));
        assert_eq!(confirmed.actions, 1);
        assert_eq!(confirmed.flags & flags::ACTION, flags::ACTION);
    }

    #[test]
    fn watchdog_neutralises_without_new_sequence() {
        let mut g = IntentGate::new(GateConfig::default());
        let t0 = Instant::now();
        let _ = g.apply(frame(7, 0.9, 1.0, 0), t0);
        let out = g.apply(frame(7, 0.9, 1.0, 0), t0 + Duration::from_millis(80));
        assert!(!out.is_valid());
        assert_eq!(out.flags & flags::GATED, flags::GATED);
    }

    #[test]
    fn artifact_is_always_gated() {
        let mut g = IntentGate::new(GateConfig::default());
        let t = Instant::now();
        let mut f = frame(1, 1.0, 1.0, 0);
        f.flags |= flags::ARTIFACT;
        let out = g.apply(f, t);
        assert_eq!(out.flags & flags::GATED, flags::GATED);
        assert!(!out.is_valid());
    }

    #[test]
    fn deadzone_activates_only_outside_band() {
        let mut engaged = false;
        assert_eq!(deadzone_axis(0.02, 0.05, 0.03, &mut engaged), 0.0);
        assert!(deadzone_axis(0.5, 0.05, 0.03, &mut engaged) > 0.4);
        assert!(deadzone_axis(-0.5, 0.05, 0.03, &mut engaged) < -0.4);
    }

    #[test]
    fn deadzone_has_hysteresis() {
        let mut engaged = false;
        // Sin engagement, 0.06 está dentro de la banda 0.05..0.08.
        assert_eq!(deadzone_axis(0.06, 0.05, 0.03, &mut engaged), 0.0);
        assert!(!engaged);
        // Un valor alto engancha el eje.
        assert!(deadzone_axis(0.5, 0.05, 0.03, &mut engaged) > 0.0);
        assert!(engaged);
        // Estando enganchado, 0.06 sigue produciendo salida.
        assert!(deadzone_axis(0.06, 0.05, 0.03, &mut engaged) > 0.0);
        // Por debajo de la zona muerta se desengancha.
        assert_eq!(deadzone_axis(0.01, 0.05, 0.03, &mut engaged), 0.0);
        assert!(!engaged);
    }

    #[test]
    fn config_rejects_inverted_thresholds() {
        let cfg = GateConfig {
            min_continuous_confidence: 0.9,
            min_discrete_confidence: 0.5,
            ..GateConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn e_stop_neutralises_immediately_and_blocks_intents() {
        let mut g = IntentGate::new(GateConfig::default());
        let t = Instant::now();
        let valid = frame(1, 0.95, 0.8, 0);
        let out1 = g.apply(valid, t);
        assert!(out1.is_valid());

        // Activamos E-Stop
        g.trigger_e_stop();
        assert!(g.is_e_stop());

        let out2 = g.apply(frame(2, 0.95, 0.8, 0), t + Duration::from_millis(10));
        assert!(!out2.is_valid());
        assert_eq!(out2.flags & flags::GATED, flags::GATED);
        assert_eq!(out2.axes[0], 0.0);

        // Restablecemos
        g.clear_e_stop();
        assert!(!g.is_e_stop());
        let out3 = g.apply(frame(3, 0.95, 0.8, 0), t + Duration::from_millis(20));
        assert!(out3.is_valid());
    }
}
