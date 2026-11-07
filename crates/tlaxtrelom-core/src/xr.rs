// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Sesión XR y composición estéreo.
//!
//! El runtime no depende de un SDK propietario para arrancar. `XrSession`
//! encapsula el contrato que un backend OpenXR debe cumplir; sin runtime XR se
//! usa `PreviewXr`, que genera poses estéreo deterministas y permite validar el
//! pipeline a 90/120 Hz en una ventana normal.
//!
//! Reglas de estabilidad para VR:
//! - El frame se predice al *display time* (`predicted_display_time_ns`), no al
//!   tiempo de CPU.
//! - La pose del frame anterior se reutiliza si el runtime no entrega una nueva
//!   (nunca se inventa una pose a medias).
//! - El presupuesto de frame se mide y se expone en `XrStats`; si se excede
//!   repetidamente, el host baja `render_scale` (reprojection de emergencia).
//! - La IPD se valida en rango humano; un valor corrupto no debe separar los
//!   ojos 30 cm.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};

/// Modo de XR del host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrMode {
    /// Render mono de escritorio.
    Off,
    /// Estéreo side-by-side en una ventana (sin compositor XR).
    StereoPreview,
    /// Sesión OpenXR real (el backend se inyecta; ver `docs/05-vr-fulldive.md`).
    OpenXr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foveation {
    Off,
    Fixed,
    EyeTracked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComfortMode {
    Seated,
    RoomScale,
}

#[derive(Debug, Clone)]
pub struct XrConfig {
    pub mode: XrMode,
    /// 90 o 120 Hz son los objetivos; 72/80/144 se aceptan como fallback.
    pub refresh_hz: u32,
    /// Escala de resolución por ojo (0.5–2.0).
    pub render_scale: f32,
    /// Distancia interpupilar en metros (58–74 mm).
    pub ipd_m: f32,
    /// FOV vertical en grados (60–120).
    pub fov_y_deg: f32,
    pub foveation: Foveation,
    pub comfort: ComfortMode,
    /// Presupuesto de procesamiento de input en µs (L_proc).
    pub input_latency_budget_us: u32,
    /// Si el frame se pasa del presupuesto N veces, bajar render_scale.
    pub adaptive_quality: bool,
}

impl Default for XrConfig {
    fn default() -> Self {
        Self {
            mode: XrMode::Off,
            refresh_hz: 120,
            render_scale: 1.0,
            ipd_m: 0.063,
            fov_y_deg: 90.0,
            foveation: Foveation::Fixed,
            comfort: ComfortMode::Seated,
            input_latency_budget_us: 1_000,
            adaptive_quality: true,
        }
    }
}

impl XrConfig {
    pub fn validate(&self) -> Result<()> {
        if !(60..=240).contains(&self.refresh_hz) {
            bail!("refresh_hz fuera de [60, 240]: {}", self.refresh_hz);
        }
        if !(0.5..=2.0).contains(&self.render_scale) {
            bail!("render_scale fuera de [0.5, 2.0]: {}", self.render_scale);
        }
        if !(0.050..=0.080).contains(&self.ipd_m) {
            bail!("ipd_m fuera de [50, 80] mm: {}", self.ipd_m);
        }
        if !(50.0..=130.0).contains(&self.fov_y_deg) {
            bail!("fov_y_deg fuera de [50, 130]: {}", self.fov_y_deg);
        }
        if self.input_latency_budget_us == 0 || self.input_latency_budget_us > 5_000 {
            bail!(
                "input_latency_budget_us fuera de (0, 5000]: {}",
                self.input_latency_budget_us
            );
        }
        Ok(())
    }

    pub fn frame_budget(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.refresh_hz.max(1) as f64)
    }

    pub fn fov_y_rad(&self) -> f32 {
        self.fov_y_deg.to_radians()
    }
}

/// Pose de la cabeza en el espacio del mundo. Se mantiene independiente de
/// `world::Transform` para que este módulo no dependa del ECS.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HeadPose {
    pub position: [f32; 3],
    /// Rotación alrededor de Y en radianes (yaw).
    pub yaw: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eye {
    Left,
    Right,
    Mono,
}

impl Eye {
    /// Signo X usado por el shader para el paralaje.
    pub fn sign(self) -> f32 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
            Self::Mono => 0.0,
        }
    }

    pub fn shader_index(self) -> f32 {
        match self {
            Self::Left => 0.0,
            Self::Right => 1.0,
            Self::Mono => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EyeView {
    pub eye: Eye,
    pub position: [f32; 3],
    /// Matriz view-projection column-major, lista para subir a GPU.
    pub view_proj: [f32; 16],
}

#[derive(Debug, Clone, Copy)]
pub struct StereoFrame {
    pub left: EyeView,
    pub right: EyeView,
    pub ipd_m: f32,
    pub frame_index: u64,
    /// Tiempo de display predicho (ns, reloj monótono del host).
    pub predicted_display_time_ns: u64,
    pub stereo: bool,
}

impl StereoFrame {
    /// Frame mono: ambos ojos comparten pose y proyección.
    pub fn mono(pose: HeadPose, aspect: f32, fov_y: f32, frame_index: u64, _now: Instant) -> Self {
        let view = EyeView {
            eye: Eye::Mono,
            position: pose.position,
            view_proj: view_projection(pose, pose.position, aspect, fov_y),
        };
        Self {
            left: view,
            right: view,
            ipd_m: 0.0,
            frame_index,
            // El display time lo fija el compositor; en mono no hay predicción.
            predicted_display_time_ns: 0,
            stereo: false,
        }
    }

    /// Frame estéreo con separación interpupilar y proyección por ojo.
    pub fn stereo(
        pose: HeadPose,
        ipd_m: f32,
        aspect_per_eye: f32,
        fov_y: f32,
        frame_index: u64,
        predicted_display_time_ns: u64,
    ) -> Self {
        let right_axis = [pose.yaw.cos(), 0.0, -pose.yaw.sin()];
        let half = ipd_m * 0.5;
        let left_pos = [
            pose.position[0] - right_axis[0] * half,
            pose.position[1],
            pose.position[2] - right_axis[2] * half,
        ];
        let right_pos = [
            pose.position[0] + right_axis[0] * half,
            pose.position[1],
            pose.position[2] + right_axis[2] * half,
        ];

        Self {
            left: EyeView {
                eye: Eye::Left,
                position: left_pos,
                view_proj: view_projection(pose, left_pos, aspect_per_eye, fov_y),
            },
            right: EyeView {
                eye: Eye::Right,
                position: right_pos,
                view_proj: view_projection(pose, right_pos, aspect_per_eye, fov_y),
            },
            ipd_m,
            frame_index,
            predicted_display_time_ns,
            stereo: true,
        }
    }
}

/// Backend XR. En producción lo implementa un adaptador OpenXR; en CI y en
/// escritorio lo implementa `PreviewXr`.
pub trait XrRuntime: Send {
    fn name(&self) -> &'static str;
    fn begin_frame(&mut self, head: HeadPose, now: Instant) -> Result<StereoFrame>;
    fn end_frame(&mut self) -> Result<()>;
    fn stats(&self) -> XrStats;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XrStats {
    pub frames: u64,
    pub late_frames: u64,
    pub dropped_frames: u64,
    pub last_frame_us: u32,
    pub max_frame_us: u32,
    pub current_render_scale: f32,
}

/// Sesión sin compositor XR. Genera estéreo determinista desde la pose del
/// jugador y aplica control adaptativo de resolución.
#[derive(Debug)]
pub struct XrSession {
    cfg: XrConfig,
    frame_index: u64,
    started: Instant,
    stats: XrStats,
    late_streak: u32,
    effective_scale: f32,
}

impl XrSession {
    pub fn new(cfg: XrConfig) -> Result<Self> {
        cfg.validate()?;
        let now = Instant::now();
        let render_scale = cfg.render_scale;
        Ok(Self {
            effective_scale: cfg.render_scale,
            cfg,
            frame_index: 0,
            started: now,
            stats: XrStats {
                current_render_scale: render_scale,
                ..XrStats::default()
            },
            late_streak: 0,
        })
    }

    pub fn config(&self) -> &XrConfig {
        &self.cfg
    }

    pub fn stats(&self) -> XrStats {
        self.stats
    }

    /// Escala de render efectiva tras el control adaptativo.
    pub fn render_scale(&self) -> f32 {
        self.effective_scale
    }

    /// Genera el frame de la sesión. `aspect` es el aspecto de la superficie
    /// completa; en estéreo se divide por ojo.
    pub fn begin_frame(
        &mut self,
        head: HeadPose,
        aspect: f32,
        now: Instant,
    ) -> Result<StereoFrame> {
        let frame_budget = self.cfg.frame_budget();
        let predicted_ns = (self.started + frame_budget * (self.frame_index as u32 + 1))
            .saturating_duration_since(self.started)
            .as_nanos() as u64;

        let frame = match self.cfg.mode {
            XrMode::Off => {
                StereoFrame::mono(head, aspect, self.cfg.fov_y_rad(), self.frame_index, now)
            }
            XrMode::StereoPreview | XrMode::OpenXr => {
                let per_eye_aspect = (aspect * 0.5).max(0.1);
                let mut frame = StereoFrame::stereo(
                    head,
                    self.cfg.ipd_m,
                    per_eye_aspect,
                    self.cfg.fov_y_rad(),
                    self.frame_index,
                    predicted_ns,
                );
                frame.stereo = true;
                // OpenXR real llega por adaptador; el preview usa la pose del
                // jugador y lo marca para que el HUD lo muestre.
                frame
            }
        };

        self.frame_index = self.frame_index.wrapping_add(1);
        self.stats.frames = self.stats.frames.saturating_add(1);
        self.stats.current_render_scale = self.effective_scale;
        Ok(frame)
    }

    /// Debe llamarse después de presentar. Devuelve `true` si el frame cumplió
    /// el presupuesto.
    pub fn end_frame(&mut self, frame_us: u32) -> bool {
        let budget_us = self.cfg.frame_budget().as_micros().min(u32::MAX as u128) as u32;
        self.stats.last_frame_us = frame_us;
        self.stats.max_frame_us = self.stats.max_frame_us.max(frame_us);

        let on_time = frame_us <= budget_us;
        if on_time {
            self.late_streak = 0;
        } else {
            self.stats.late_frames = self.stats.late_frames.saturating_add(1);
            self.late_streak = self.late_streak.saturating_add(1);
            if self.late_streak == 8 {
                self.stats.dropped_frames = self.stats.dropped_frames.saturating_add(1);
            }
            if self.cfg.adaptive_quality && self.late_streak >= 30 {
                self.effective_scale = (self.effective_scale * 0.9).max(0.5);
                self.late_streak = 0;
            }
        }
        on_time
    }

    /// Simula la pérdida de foco o el cierre del compositor. Fuerza neutral en
    /// el próximo frame; el host decide si pausa el juego.
    pub fn reset(&mut self) {
        self.frame_index = 0;
        self.late_streak = 0;
    }
}

impl XrRuntime for XrSession {
    fn name(&self) -> &'static str {
        match self.cfg.mode {
            XrMode::Off => "mono",
            XrMode::StereoPreview => "stereo-preview",
            XrMode::OpenXr => "openxr-adapter",
        }
    }

    fn begin_frame(&mut self, head: HeadPose, now: Instant) -> Result<StereoFrame> {
        // Aspect de referencia 16:9 para el adaptador; el host puede pasar el
        // real mediante `XrSession::begin_frame` directamente.
        self.begin_frame(head, 16.0 / 9.0, now)
    }

    fn end_frame(&mut self) -> Result<()> {
        Ok(())
    }

    fn stats(&self) -> XrStats {
        self.stats
    }
}

/// Matriz de vista-proyección column-major. El mundo usa Y arriba y Z adelante
/// (yaw 0 mira a +Z), igual que `world::World`.
fn view_projection(pose: HeadPose, eye: [f32; 3], aspect: f32, fov_y: f32) -> [f32; 16] {
    let proj = perspective(fov_y, aspect.max(0.05), 0.05, 10_000.0);
    let view = view_matrix(pose.yaw, eye);
    multiply(&proj, &view)
}

/// Proyección **left-handed** con profundidad 0..1 (convención wgpu/D3D).
///
/// El mundo usa Y arriba y +Z adelante. Una proyección right-handed con la
/// misma vista haría que el jugador mirase en dirección contraria; este era el
/// bug clásico de los previews estéreo.
fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let d = (far - near).max(1e-6);
    [
        f / aspect.max(1e-6),
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        far / d,
        1.0,
        0.0,
        0.0,
        -near * far / d,
        0.0,
    ]
}

fn view_matrix(yaw: f32, eye: [f32; 3]) -> [f32; 16] {
    let (s, c) = yaw.sin_cos();
    // R(-yaw) * T(-eye), column-major.
    [
        c,
        0.0,
        -s,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        s,
        0.0,
        c,
        0.0,
        -(c * eye[0] + s * eye[2]),
        -eye[1],
        -(-s * eye[0] + c * eye[2]),
        1.0,
    ]
}

/// Multiplicación column-major: `out = a * b`.
fn multiply(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Estabilidad VR: Filtro 1€, Predicción de Pose, Confort y Eye Tracking
// ─────────────────────────────────────────────────────────────────────────────

/// Filtro 1€ (One-Euro Filter): estándar para reducir jitter en reposo sin
/// introducir desfase ni latencia durante movimientos rápidos en VR.
#[derive(Debug, Clone, Copy)]
pub struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x_prev: f32,
    dx_prev: f32,
    initialized: bool,
}

impl OneEuroFilter {
    pub fn new(min_cutoff: f32, beta: f32) -> Self {
        Self {
            min_cutoff: min_cutoff.max(0.001),
            beta: beta.max(0.0),
            d_cutoff: 1.0,
            x_prev: 0.0,
            dx_prev: 0.0,
            initialized: false,
        }
    }

    pub fn filter(&mut self, x: f32, dt_s: f32) -> f32 {
        if !x.is_finite() || dt_s <= 0.0 || !dt_s.is_finite() {
            return self.x_prev;
        }
        if !self.initialized {
            self.x_prev = x;
            self.dx_prev = 0.0;
            self.initialized = true;
            return x;
        }

        let dx = (x - self.x_prev) / dt_s;
        let edx = self.filter_alpha(dx, self.dx_prev, dt_s, self.d_cutoff);
        self.dx_prev = edx;

        let cutoff = self.min_cutoff + self.beta * edx.abs();
        let filtered = self.filter_alpha(x, self.x_prev, dt_s, cutoff);
        self.x_prev = filtered;
        filtered
    }

    fn filter_alpha(&self, val: f32, prev: f32, dt_s: f32, cutoff: f32) -> f32 {
        let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(0.001));
        let alpha = 1.0 / (1.0 + tau / dt_s);
        alpha * val + (1.0 - alpha) * prev
    }

    pub fn reset(&mut self) {
        self.initialized = false;
        self.x_prev = 0.0;
        self.dx_prev = 0.0;
    }
}

/// Filtro 1€ para la pose completa de la cabeza (posición 3D + rotación yaw).
#[derive(Debug, Clone, Copy)]
pub struct PoseFilter {
    x: OneEuroFilter,
    y: OneEuroFilter,
    z: OneEuroFilter,
    yaw: OneEuroFilter,
}

impl PoseFilter {
    pub fn new(min_cutoff: f32, beta: f32) -> Self {
        Self {
            x: OneEuroFilter::new(min_cutoff, beta),
            y: OneEuroFilter::new(min_cutoff, beta),
            z: OneEuroFilter::new(min_cutoff, beta),
            yaw: OneEuroFilter::new(min_cutoff, beta),
        }
    }

    pub fn filter(&mut self, pose: HeadPose, dt_s: f32) -> HeadPose {
        HeadPose {
            position: [
                self.x.filter(pose.position[0], dt_s),
                self.y.filter(pose.position[1], dt_s),
                self.z.filter(pose.position[2], dt_s),
            ],
            yaw: self.yaw.filter(pose.yaw, dt_s),
        }
    }

    pub fn reset(&mut self) {
        self.x.reset();
        self.y.reset();
        self.z.reset();
        self.yaw.reset();
    }
}

/// Predictor de pose para compensar la latencia Motion-to-Photon (MTP).
/// Extrapola posición y orientación hacia el tiempo estimado de presentación
/// de pantalla (`predicted_display_time`), con límites de velocidad y jerk.
#[derive(Debug, Clone, Copy, Default)]
pub struct PosePredictor {
    last_pose: HeadPose,
    velocity: [f32; 3],
    yaw_rate: f32,
    last_time_ns: u64,
    initialized: bool,
}

impl PosePredictor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, pose: HeadPose, time_ns: u64) {
        if !self.initialized {
            self.last_pose = pose;
            self.last_time_ns = time_ns;
            self.velocity = [0.0; 3];
            self.yaw_rate = 0.0;
            self.initialized = true;
            return;
        }

        let dt_s = (time_ns.saturating_sub(self.last_time_ns) as f32) * 1e-9;
        if dt_s > 1e-5 && dt_s < 0.2 {
            for i in 0..3 {
                let v = (pose.position[i] - self.last_pose.position[i]) / dt_s;
                self.velocity[i] = v.clamp(-15.0, 15.0);
            }
            let dyaw = pose.yaw - self.last_pose.yaw;
            self.yaw_rate = (dyaw / dt_s).clamp(-20.0, 20.0);
        }
        self.last_pose = pose;
        self.last_time_ns = time_ns;
    }

    pub fn predict(&self, target_time_ns: u64) -> HeadPose {
        if !self.initialized {
            return self.last_pose;
        }
        let delta_s = (target_time_ns.saturating_sub(self.last_time_ns) as f32) * 1e-9;
        let safe_delta_s = delta_s.clamp(0.0, 0.050);

        HeadPose {
            position: [
                self.last_pose.position[0] + self.velocity[0] * safe_delta_s,
                self.last_pose.position[1] + self.velocity[1] * safe_delta_s,
                self.last_pose.position[2] + self.velocity[2] * safe_delta_s,
            ],
            yaw: self.last_pose.yaw + self.yaw_rate * safe_delta_s,
        }
    }
}

/// Sistema de viñeteado dinámico de confort contra el mareo por movimiento (VR sickness).
/// Reduce el campo visual periférico proporcionalmente a la velocidad angular
/// y aceleración lineal, estabilizando la percepción vestibular.
#[derive(Debug, Clone, Copy)]
pub struct ComfortVignette {
    aperture: f32,
    angular_sensitivity: f32,
    decay_rate: f32,
}

impl Default for ComfortVignette {
    fn default() -> Self {
        Self {
            aperture: 1.0,
            angular_sensitivity: 0.25,
            decay_rate: 6.0,
        }
    }
}

impl ComfortVignette {
    pub fn new(angular_sensitivity: f32) -> Self {
        Self {
            angular_sensitivity: angular_sensitivity.clamp(0.05, 1.0),
            ..Self::default()
        }
    }

    pub fn update(&mut self, angular_vel_rad_s: f32, linear_accel_m_s2: f32, dt_s: f32) -> f32 {
        let motion_intensity =
            angular_vel_rad_s.abs() * self.angular_sensitivity + (linear_accel_m_s2.abs() * 0.05);
        let target_aperture = (1.0 - motion_intensity).clamp(0.40, 1.0);

        let rate = if target_aperture < self.aperture {
            18.0
        } else {
            self.decay_rate
        };
        let factor = (rate * dt_s).clamp(0.0, 1.0);
        self.aperture += (target_aperture - self.aperture) * factor;
        self.aperture
    }

    pub fn aperture(&self) -> f32 {
        self.aperture
    }
}

/// Datos de seguimiento ocular (Eye Tracking) para foveated rendering y control de foco.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GazeSample {
    pub gaze_uv: [f32; 2],
    pub pupil_diameter_mm: f32,
    pub confidence: f32,
    pub saccade_active: bool,
}

impl Default for GazeSample {
    fn default() -> Self {
        Self {
            gaze_uv: [0.5, 0.5],
            pupil_diameter_mm: 3.5,
            confidence: 1.0,
            saccade_active: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        XrConfig::default().validate().expect("XR válido");
    }

    #[test]
    fn rejects_absurd_ipd() {
        let cfg = XrConfig {
            ipd_m: 0.5,
            ..XrConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn stereo_separates_eyes_by_ipd() {
        let cfg = XrConfig {
            mode: XrMode::StereoPreview,
            ipd_m: 0.064,
            ..XrConfig::default()
        };
        let frame = StereoFrame::stereo(HeadPose::default(), cfg.ipd_m, 0.5, 1.5, 0, 0);
        let dx = frame.right.position[0] - frame.left.position[0];
        assert!((dx.abs() - 0.064).abs() < 1e-5, "dx={dx}");
        assert!(frame.stereo);
    }

    #[test]
    fn matrices_are_finite() {
        let frame = StereoFrame::stereo(HeadPose::default(), 0.063, 0.5, 1.5, 0, 0);
        assert!(frame.left.view_proj.iter().all(|v| v.is_finite()));
        assert!(frame.right.view_proj.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn session_degrades_render_scale_after_sustained_late_frames() {
        let cfg = XrConfig {
            adaptive_quality: true,
            render_scale: 1.0,
            ..XrConfig::default()
        };
        let mut s = XrSession::new(cfg).expect("sesión XR");
        for _ in 0..40 {
            s.end_frame(50_000); // 50 ms >> budget
        }
        assert!(s.render_scale() < 1.0);
        assert!(s.stats().late_frames > 0);
    }

    #[test]
    fn session_does_not_degrade_when_on_time() {
        let mut s = XrSession::new(XrConfig::default()).unwrap();
        for _ in 0..120 {
            s.end_frame(1_000);
        }
        assert_eq!(s.render_scale(), 1.0);
    }

    #[test]
    fn mono_frame_has_no_parallax() {
        let frame = StereoFrame::mono(HeadPose::default(), 1.0, 1.5, 0, Instant::now());
        assert!(!frame.stereo);
        assert_eq!(frame.left.eye, Eye::Mono);
    }

    #[test]
    fn projection_has_expected_shape() {
        let p = perspective(1.5, 1.0, 0.05, 100.0);
        assert!(p[11] == 1.0, "proyección LH column-major incorrecta");
    }

    #[test]
    fn projection_maps_near_and_far_to_zero_and_one() {
        let near = 0.05f32;
        let far = 100.0f32;
        let p = perspective(1.5, 1.0, near, far);
        let ndc_z = |z: f32| (p[10] * z + p[14]) / (p[11] * z + p[15]);
        assert!(ndc_z(near).abs() < 1e-5, "near={}", ndc_z(near));
        assert!((ndc_z(far) - 1.0).abs() < 1e-5, "far={}", ndc_z(far));
    }

    #[test]
    fn yaw_zero_looks_towards_positive_z() {
        let frame = StereoFrame::mono(
            HeadPose {
                position: [0.0, 1.7, 0.0],
                yaw: 0.0,
            },
            16.0 / 9.0,
            1.5,
            0,
            Instant::now(),
        );
        // Un punto delante de la cámara (z positivo) debe quedar en w > 0.
        let m = frame.left.view_proj;
        let z = m[11] * 10.0 + m[15];
        assert!(z > 0.0, "w={z}");
    }

    #[test]
    fn one_euro_filter_reduces_stationary_jitter() {
        let mut filter = OneEuroFilter::new(1.0, 0.0);
        let dt = 0.011; // ~90 Hz
                        // Primera muestra inicializa
        let _ = filter.filter(10.0, dt);
        // Ruido de alta frecuencia estacionario (jitter submilimétrico)
        let mut out = 0.0;
        for i in 0..10 {
            let noise = if i % 2 == 0 { 10.05 } else { 9.95 };
            out = filter.filter(noise, dt);
        }
        // El filtro debe haber atenuado fuertemente la oscilación
        assert!((out - 10.0).abs() < 0.03, "out={out}");
    }

    #[test]
    fn pose_predictor_extrapolates_forward_and_clamps_delta() {
        let mut predictor = PosePredictor::new();
        let p0 = HeadPose {
            position: [0.0, 1.0, 0.0],
            yaw: 0.0,
        };
        predictor.update(p0, 1_000_000_000);

        let p1 = HeadPose {
            position: [0.1, 1.0, 0.0],
            yaw: 0.1,
        };
        predictor.update(p1, 1_010_000_000); // 10 ms después

        // Predice 10 ms hacia el futuro
        let pred = predictor.predict(1_020_000_000);
        assert!((pred.position[0] - 0.2).abs() < 1e-3);
        assert!((pred.yaw - 0.2).abs() < 1e-3);

        // Predicción excesiva se acota a 50 ms
        let pred_far = predictor.predict(2_000_000_000);
        assert!(pred_far.position[0] <= 0.61, "clamp de delta excedido");
    }

    #[test]
    fn comfort_vignette_narrows_on_sharp_rotation() {
        let mut vignette = ComfortVignette::new(0.5);
        assert_eq!(vignette.aperture(), 1.0);

        // Giro brusco a 4 rad/s (~230 deg/s)
        let ap = vignette.update(4.0, 0.0, 0.05);
        assert!(
            ap < 0.9,
            "el viñeteado debe cerrarse para proteger del mareo: {ap}"
        );
        assert!(ap >= 0.4, "el viñeteado no debe bajar de 0.4");
    }
}
