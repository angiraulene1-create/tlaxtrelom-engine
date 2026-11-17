// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
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

use crate::dimension::DimensionMode;

/// Rango de profundidad de la proyección perspectiva, en metros.
///
/// En un mundo partido en *sectores* el `far` es el tamaño del sector, no una
/// constante global: un sector de 20 km necesita otro `near` que uno de 100 m,
/// porque el depth buffer reparte su resolución en el rango `[near, far]` y un
/// `near` diminuto con un `far` enorme deja todo el detalle en el lejano (z-fighting
/// en primer plano).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthRange {
    pub near_m: f32,
    pub far_m: f32,
}

impl Default for DepthRange {
    fn default() -> Self {
        Self {
            near_m: 0.05,
            far_m: 10_000.0,
        }
    }
}

impl DepthRange {
    pub fn new(near_m: f32, far_m: f32) -> Self {
        Self { near_m, far_m }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.near_m.is_finite() || !self.far_m.is_finite() {
            bail!("rango de profundidad no finito: {self:?}");
        }
        if self.near_m <= 0.0 || self.near_m > 200.0 {
            bail!("near_m fuera de (0, 200] m: {}", self.near_m);
        }
        // Un `far` por encima de 1e8 m no tiene sentido ni siquiera para un
        // sector: lo que está más lejos se dibuja como cielo, no como geometría.
        if self.far_m <= self.near_m || self.far_m > 1e8 {
            bail!(
                "far_m fuera de (near_m, 1e8] m o menor que near_m: {}",
                self.far_m
            );
        }
        Ok(())
    }

    /// Rango utilizable para una matriz: finito y con separación mínima.
    /// `validate` rechaza la configuración inválida en el arranque, pero la
    /// proyección también se construye desde `StereoFrame::*` públicos y un NaN
    /// en `view_proj` tira el frame en la GPU sin mensaje útil.
    fn clamped(self) -> Self {
        let near_m = if self.near_m.is_finite() {
            self.near_m.clamp(0.001, 200.0)
        } else {
            Self::default().near_m
        };
        let far_m = if self.far_m.is_finite() {
            self.far_m.max(near_m * 10.0).min(1e8)
        } else {
            Self::default().far_m
        };
        Self { near_m, far_m }
    }

    /// Parámetros de niebla del shader: `[inicio, rampa, densidad máxima, 0]`
    /// en metros, listos para el `vec4` de `SceneUniforms`.
    ///
    /// Escalan con el alcance del sector en vez de ser un número mágico: en un
    /// sector de 40 km la niebla debe aparecer más lejos, no tragar la escena a
    /// 65 m igual que en uno de 10 km. Con el alcance por defecto reproduce
    /// exactamente los valores históricos (10 m / 55 m / 0.78).
    pub fn fog_params(&self) -> [f32; 4] {
        let start = (self.clamped().far_m * 0.001).clamp(8.0, 400.0);
        [start, start * 5.5, 0.78, 0.0]
    }
}

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
    /// Modo de presentación dimensional (2D…5D). Afecta a la proyección y a
    /// la geometría hiperdimensional dibujada sobre la escena 3D.
    pub dimension: DimensionMode,
    /// Alcance del sector: rango `[near, far]` de la proyección en metros.
    pub depth: DepthRange,
    /// Fuerza de perspectiva del modo `3.5d` (ver `Projection::perspective_blend`).
    pub perspective_blend: f32,
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
            dimension: DimensionMode::Spatial3D,
            depth: DepthRange::default(),
            perspective_blend: 0.5,
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
        self.depth.validate()?;
        if !(0.0..=1.0).contains(&self.perspective_blend) || !self.perspective_blend.is_finite() {
            bail!(
                "perspective_blend fuera de [0, 1]: {}",
                self.perspective_blend
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

/// Cómo se encuadra un frame: relación de aspecto por ojo, FOV vertical y
/// alcance del sector. Viajan juntos porque la proyección y la niebla tienen
/// que ser coherentes entre sí.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub aspect: f32,
    pub fov_y_rad: f32,
    pub depth: DepthRange,
    /// Fuerza de perspectiva del modo `3.5d`: 0 = la ortográfica del `2.5d`,
    /// 1 = perspectiva plena. Los demás modos no la leen.
    pub perspective_blend: f32,
}

impl Projection {
    /// Proyección con el alcance de sector por defecto.
    pub fn new(aspect: f32, fov_y_rad: f32) -> Self {
        Self {
            aspect,
            fov_y_rad,
            depth: DepthRange::default(),
            perspective_blend: 0.5,
        }
    }

    /// Copia con la fuerza de perspectiva de `3.5d` fijada.
    pub fn with_blend(mut self, perspective_blend: f32) -> Self {
        self.perspective_blend = perspective_blend;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StereoFrame {
    pub left: EyeView,
    pub right: EyeView,
    pub ipd_m: f32,
    /// `[inicio, rampa, densidad máxima, 0]` derivada del alcance del sector: la
    /// sube el renderer tal cual, sin volver a consultar la configuración.
    pub fog: [f32; 4],
    /// Apertura de confort [0.4, 1.0] de [`ComfortVignette`]: la fija
    /// `XrSession::begin_frame` según la velocidad angular del frame anterior
    /// y `scene3d.wgsl` la convierte en viñeteado anti-cinetosis. 1.0 = sin
    /// viñeta.
    pub comfort_aperture: f32,
    pub frame_index: u64,
    /// Tiempo de display predicho (ns, reloj monótono del host).
    pub predicted_display_time_ns: u64,
    pub stereo: bool,
}

impl StereoFrame {
    /// Frame mono: ambos ojos comparten pose y proyección.
    pub fn mono(pose: HeadPose, aspect: f32, fov_y: f32, frame_index: u64, now: Instant) -> Self {
        Self::mono_with_dimension(
            DimensionMode::Spatial3D,
            pose,
            Projection::new(aspect, fov_y),
            frame_index,
            now,
        )
    }

    /// Frame mono con modo dimensional y alcance de sector explícitos.
    ///
    /// `pose.position` debe ser el **offset local** respecto del origen de
    /// render (ver [`crate::coords`]), nunca una coordenada global: la IPD y la
    /// matriz de vista se suman aquí, y en coordenadas absolutas `f32` esas
    /// sumas pierden el milímetro más allá de ~16 km y desaparecen del todo
    /// cerca del millón de metros.
    pub fn mono_with_dimension(
        dimension: DimensionMode,
        pose: HeadPose,
        proj: Projection,
        frame_index: u64,
        _now: Instant,
    ) -> Self {
        let view = EyeView {
            eye: Eye::Mono,
            position: pose.position,
            view_proj: dimension_view_projection(dimension, pose, pose.position, proj),
        };
        Self {
            left: view,
            right: view,
            ipd_m: 0.0,
            fog: proj.depth.fog_params(),
            comfort_aperture: 1.0,
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
        Self::stereo_with_dimension(
            DimensionMode::Spatial3D,
            pose,
            ipd_m,
            Projection::new(aspect_per_eye, fov_y),
            frame_index,
            predicted_display_time_ns,
        )
    }

    /// Frame estéreo con modo dimensional y alcance de sector explícitos.
    pub fn stereo_with_dimension(
        dimension: DimensionMode,
        pose: HeadPose,
        ipd_m: f32,
        proj: Projection,
        frame_index: u64,
        predicted_display_time_ns: u64,
    ) -> Self {
        // La IPD se suma al offset local del jugador. Sumarla a una coordenada
        // global `f32` la redondearía a cero cerca del millón de metros del
        // origen (y le quitaría precisión mucho antes): el estéreo colapsaría a
        // mono sin ningún error visible.
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
                view_proj: dimension_view_projection(dimension, pose, left_pos, proj),
            },
            right: EyeView {
                eye: Eye::Right,
                position: right_pos,
                view_proj: dimension_view_projection(dimension, pose, right_pos, proj),
            },
            ipd_m,
            fog: proj.depth.fog_params(),
            comfort_aperture: 1.0,
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
    vignette: ComfortVignette,
    last_yaw: f32,
    last_pose_time: Option<Instant>,
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
            vignette: ComfortVignette::default(),
            last_yaw: 0.0,
            last_pose_time: None,
        })
    }

    /// Proyección del frame a partir de la configuración de la sesión.
    fn projection(&self, aspect: f32) -> Projection {
        Projection {
            aspect,
            fov_y_rad: self.cfg.fov_y_rad(),
            depth: self.cfg.depth,
            perspective_blend: self.cfg.perspective_blend,
        }
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

        // La apertura de confort mide la velocidad angular sobre el delta de
        // yaw desenvuelto: la resta cruda saltaría ±π en cada vuelta y la
        // viñeta se cerraría sola en reposo.
        let aperture = match self.last_pose_time {
            Some(prev) => {
                let dt = now.saturating_duration_since(prev).as_secs_f32().max(1e-3);
                let dyaw = wrap_angle(head.yaw - self.last_yaw);
                self.vignette.update(dyaw / dt, 0.0, dt)
            }
            None => self.vignette.aperture(),
        };
        self.last_pose_time = Some(now);
        self.last_yaw = head.yaw;

        let mut frame = match self.cfg.mode {
            XrMode::Off => StereoFrame::mono_with_dimension(
                self.cfg.dimension,
                head,
                self.projection(aspect),
                self.frame_index,
                now,
            ),
            XrMode::StereoPreview | XrMode::OpenXr => {
                let per_eye_aspect = (aspect * 0.5).max(0.1);
                let mut frame = StereoFrame::stereo_with_dimension(
                    self.cfg.dimension,
                    head,
                    self.cfg.ipd_m,
                    self.projection(per_eye_aspect),
                    self.frame_index,
                    predicted_ns,
                );
                frame.stereo = true;
                // OpenXR real llega por adaptador; el preview usa la pose del
                // jugador y lo marca para que el HUD lo muestre.
                frame
            }
        };
        frame.comfort_aperture = aperture;

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
fn view_projection(pose: HeadPose, eye: [f32; 3], proj: Projection) -> [f32; 16] {
    let depth = proj.depth.clamped();
    let p = perspective(
        proj.fov_y_rad,
        proj.aspect.max(0.05),
        depth.near_m,
        depth.far_m,
    );
    let view = view_matrix(pose.yaw, eye);
    multiply(&p, &view)
}

/// Proyección según el modo dimensional.
///
/// - `2D`: identidad (la escena ya está en NDC); un desplazamiento lateral
///   mínimo por ojo conserva algo de paralaje en estéreo.
/// - `2.5D`: cámara ortográfica isométrica con inclinación de 35° que sigue el
///   offset del jugador.
/// - `3.5D`: el mismo encuadre isométrico con la perspectiva mezclada según
///   `Projection::perspective_blend`.
/// - `3D`, `4D`, `4.5D`, `5D`: perspectiva normal. Los ejes ocultos ya fueron
///   colapsados a 3D por `dimension::project` antes de llegar aquí.
fn dimension_view_projection(
    dimension: DimensionMode,
    pose: HeadPose,
    eye: [f32; 3],
    proj: Projection,
) -> [f32; 16] {
    let right_axis = [pose.yaw.cos(), 0.0, -pose.yaw.sin()];
    let lateral =
        (eye[0] - pose.position[0]) * right_axis[0] + (eye[2] - pose.position[2]) * right_axis[2];

    match dimension {
        DimensionMode::Flat2D => flat_view_projection(lateral * 0.35),
        DimensionMode::Isometric25D => {
            isometric_view_projection(proj.aspect, lateral * 1.5, pose.position)
        }
        DimensionMode::Fractional35D => blended_view_projection(
            proj.aspect,
            lateral * 1.5,
            pose.position,
            proj.perspective_blend,
        ),
        _ => view_projection(pose, eye, proj),
    }
}

/// Vista plana 2D: identidad salvo un desplazamiento horizontal para estéreo.
fn flat_view_projection(shift_x: f32) -> [f32; 16] {
    let sx = if shift_x.is_finite() {
        shift_x.clamp(-0.05, 0.05)
    } else {
        0.0
    };
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, sx, 0.0, 0.0, 1.0,
    ]
}

/// Encuadre isométrico compartido por `2.5D` y `3.5D`: volumen y cámara fijos,
/// centrados en el **offset** del jugador (nunca en el origen global).
const ISO_HALF_HEIGHT: f32 = 11.0;
const ISO_NEAR: f32 = 0.1;
const ISO_FAR: f32 = 500.0;

/// Ojo y objetivo de la cámara isométrica que sigue el offset local.
fn isometric_eye_target(focus: [f32; 3], lateral: f32) -> ([f32; 3], [f32; 3]) {
    let eye = [focus[0] - 14.0 + lateral, focus[1] + 11.0, focus[2] - 10.0];
    let target = [focus[0], focus[1], focus[2] + 8.0];
    (eye, target)
}

/// Vista isométrica para 2.5D: cámara elevada que sigue el **offset** del
/// jugador.
///
/// El objetivo no puede ser el origen global: con origen flotante el jugador
/// puede estar a kilómetros de él y la cámara ortográfica lo dejaría fuera de
/// cuadro. `focus` son las coordenadas locales, siempre cercanas a cero.
fn isometric_view_projection(aspect: f32, lateral: f32, focus: [f32; 3]) -> [f32; 16] {
    let proj = orthographic(aspect, ISO_HALF_HEIGHT, ISO_NEAR, ISO_FAR);
    let (eye, target) = isometric_eye_target(focus, lateral);
    let view = look_at_forward(eye, target, [0.0, 1.0, 0.0]);
    multiply(&proj, &view)
}

/// Vista 3.5D: el encuadre isométrico del 2.5D con perspectiva parcial.
///
/// Es un `lerp` elemento a elemento entre la matriz ortográfica y una
/// perspectiva cuya focal está calibrada para dar **el mismo tamaño a la
/// distancia del foco**. Por eso mezclar no mueve el encuadre: con `blend = 0`
/// la matriz es exactamente la de `2.5D`, con `blend = 1` es perspectiva plena
/// del mismo cuadro, y en el medio los objetos cercanos crecen y los lejanos
/// se encogen sin que la imagen salte.
fn blended_view_projection(aspect: f32, lateral: f32, focus: [f32; 3], blend: f32) -> [f32; 16] {
    let aspect = aspect.max(0.05);
    let (eye, target) = isometric_eye_target(focus, lateral);
    let view = look_at_forward(eye, target, [0.0, 1.0, 0.0]);
    let ortho = orthographic(aspect, ISO_HALF_HEIGHT, ISO_NEAR, ISO_FAR);
    // Distancia de la cámara al foco: donde las dos proyecciones coinciden.
    let dist = length3(sub3(target, eye)).max(2.0 * ISO_HALF_HEIGHT);
    let persp = perspective(
        2.0 * (ISO_HALF_HEIGHT / dist).atan(),
        aspect,
        ISO_NEAR,
        ISO_FAR,
    );
    multiply(&blend_projection(&ortho, &persp, blend), &view)
}

/// Interpolación lineal entre dos matrices de proyección. Un `lerp` de
/// matrices es un `lerp` de sus resultados en espacio clip (la multiplicación
/// por un punto es lineal), así que el único cuidado es el `w`.
fn blend_projection(a: &[f32; 16], b: &[f32; 16], blend: f32) -> [f32; 16] {
    let t = if blend.is_finite() {
        blend.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let mut out = [0.0f32; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = a[i] + (b[i] - a[i]) * t;
    }
    out
}

/// Proyección **left-handed** con profundidad 0..1 (convención wgpu/D3D).
///
/// El mundo usa Y arriba y +Z adelante. Una proyección right-handed con la
/// misma vista haría que el jugador mirase en dirección contraria; este era el
/// bug clásico de los previews estéreo.
pub(crate) fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
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

/// Proyección ortográfica **left-handed** con profundidad 0..1.
fn orthographic(aspect: f32, half_height: f32, near: f32, far: f32) -> [f32; 16] {
    let hh = half_height.max(0.1);
    let hw = hh * aspect.max(0.05);
    let d = (far - near).max(1e-6);
    [
        1.0 / hw,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0 / hh,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0 / d,
        0.0,
        0.0,
        0.0,
        -near / d,
        1.0,
    ]
}

/// Matriz de vista left-handed para una cámara que mira hacia `target`.
/// Se usa en el modo isométrico 2.5D, donde no interesa la pose del jugador.
fn look_at_forward(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let forward = normalize3(sub3(target, eye));
    let mut right = normalize3(cross3(up, forward));
    if length3(right) < 1e-6 {
        right = [1.0, 0.0, 0.0];
    }
    let up2 = cross3(forward, right);
    [
        right[0],
        up2[0],
        forward[0],
        0.0,
        right[1],
        up2[1],
        forward[1],
        0.0,
        right[2],
        up2[2],
        forward[2],
        0.0,
        -dot3(right, eye),
        -dot3(up2, eye),
        -dot3(forward, eye),
        1.0,
    ]
}

pub(crate) fn view_matrix(yaw: f32, eye: [f32; 3]) -> [f32; 16] {
    let (s, c) = yaw.sin_cos();
    // R(-yaw) * T(-eye), column-major. Con la proyección left-handed (+Z
    // adelante), yaw = +90° mira hacia +X, igual que el movimiento del mundo.
    [
        c,
        0.0,
        s,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        -s,
        0.0,
        c,
        0.0,
        -c * eye[0] + s * eye[2],
        -eye[1],
        -s * eye[0] - c * eye[2],
        1.0,
    ]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length3(a: [f32; 3]) -> f32 {
    dot3(a, a).sqrt()
}

fn normalize3(a: [f32; 3]) -> [f32; 3] {
    let len = length3(a);
    if len.is_finite() && len > 1e-8 {
        [a[0] / len, a[1] / len, a[2] / len]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Multiplicación column-major: `out = a * b`.
pub(crate) fn multiply(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
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
    /// Último yaw continuo (sin el salto de ±π) usado como referencia para
    /// desenrollar la entrada antes de filtrarla.
    yaw_ref: f32,
}

impl PoseFilter {
    pub fn new(min_cutoff: f32, beta: f32) -> Self {
        Self {
            x: OneEuroFilter::new(min_cutoff, beta),
            y: OneEuroFilter::new(min_cutoff, beta),
            z: OneEuroFilter::new(min_cutoff, beta),
            yaw: OneEuroFilter::new(min_cutoff, beta),
            yaw_ref: 0.0,
        }
    }

    pub fn filter(&mut self, pose: HeadPose, dt_s: f32) -> HeadPose {
        // El filtro 1€ es lineal: filtrar el yaw tal cual cruza ±π produciría
        // un barrido por 0 (o por ±2π) que el usuario percibe como un giro
        // brusco. Se desenrolla respecto al último valor continuo y se vuelve
        // a envolver a [-π, π] a la salida.
        let yaw = unwrap_near(pose.yaw, self.yaw_ref);
        self.yaw_ref = yaw;
        HeadPose {
            position: [
                self.x.filter(pose.position[0], dt_s),
                self.y.filter(pose.position[1], dt_s),
                self.z.filter(pose.position[2], dt_s),
            ],
            yaw: wrap_angle(self.yaw.filter(yaw, dt_s)),
        }
    }

    pub fn reset(&mut self) {
        self.x.reset();
        self.y.reset();
        self.z.reset();
        self.yaw.reset();
        self.yaw_ref = 0.0;
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
        // Una pose no finita no debe envenenar el predictor ni las matrices del
        // compositor: se descarta y se conserva la última pose válida.
        if pose.position.iter().any(|v| !v.is_finite()) || !pose.yaw.is_finite() {
            return;
        }
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
            let dyaw = wrap_angle(pose.yaw - self.last_pose.yaw);
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
            yaw: wrap_angle(self.last_pose.yaw + self.yaw_rate * safe_delta_s),
        }
    }
}

/// Normaliza un ángulo a `[-π, π]`. Un valor no finito se trata como 0 para no
/// propagar `NaN` a matrices ni a la predicción.
fn wrap_angle(a: f32) -> f32 {
    if !a.is_finite() {
        return 0.0;
    }
    let mut wrapped = a % std::f32::consts::TAU;
    if wrapped > std::f32::consts::PI {
        wrapped -= std::f32::consts::TAU;
    } else if wrapped < -std::f32::consts::PI {
        wrapped += std::f32::consts::TAU;
    }
    wrapped
}

/// Representa `angle` como el equivalente más cercano a `reference`, evitando
/// el salto de discontinuidad de ±π.
fn unwrap_near(angle: f32, reference: f32) -> f32 {
    if !angle.is_finite() {
        return reference;
    }
    reference + wrap_angle(angle - reference)
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
    fn la_sesion_estrecha_la_vineta_al_cerrar_y_la_abre_al_asentar() {
        let mut s = XrSession::new(XrConfig::default()).expect("sesión XR");
        let t0 = Instant::now();
        let pose = |yaw| HeadPose {
            position: [0.0, 1.6, 0.0],
            yaw,
        };
        // Primer frame: sin histórico, apertura de reposo.
        assert_eq!(
            s.begin_frame(pose(0.0), 1.6, t0).unwrap().comfort_aperture,
            1.0
        );
        // 2 rad en 50 ms es un latigazo: la apertura debe bajar.
        let whip = s
            .begin_frame(pose(2.0), 1.6, t0 + Duration::from_millis(50))
            .unwrap()
            .comfort_aperture;
        assert!(whip < 1.0, "giro brusco sin viñeta: {whip}");
        assert!(
            (0.4..1.0).contains(&whip),
            "apertura fuera de rango: {whip}"
        );
        // Con la cabeza asentada la viñeta se retira hacia 1.0.
        let mut settled = whip;
        for i in 1..200 {
            let next = s
                .begin_frame(pose(2.0), 1.6, t0 + Duration::from_millis(50 + i * 100))
                .unwrap()
                .comfort_aperture;
            assert!(next >= settled - 1e-6, "la viñeta se cerró estando quieta");
            settled = next;
        }
        assert!(
            settled > 0.95,
            "la viñeta no se abrió al asentar: {settled}"
        );
    }

    #[test]
    fn el_delta_de_yaw_se_mide_desenvuelto_sin_salto_de_pi() {
        let mut s = XrSession::new(XrConfig::default()).expect("sesión XR");
        let t0 = Instant::now();
        let pose = |yaw| HeadPose {
            position: [0.0, 1.6, 0.0],
            yaw,
        };
        s.begin_frame(pose(std::f32::consts::PI - 0.01), 1.6, t0)
            .unwrap();
        // Cruzar ±π es medio centiradián de giro real, no una resta de ~2π
        // que cerraría la viñeta con la cabeza quieta.
        let calm = s
            .begin_frame(
                pose(-std::f32::consts::PI + 0.01),
                1.6,
                t0 + Duration::from_millis(200),
            )
            .unwrap()
            .comfort_aperture;
        assert!(calm > 0.97, "el salto de ±π cerró la viñeta: {calm}");
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

    fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 4] {
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15],
        ]
    }

    #[test]
    fn yaw_plus_90_degrees_looks_towards_positive_x() {
        let frame = StereoFrame::mono(
            HeadPose {
                position: [0.0, 0.0, 0.0],
                yaw: std::f32::consts::FRAC_PI_2,
            },
            1.0,
            1.5,
            0,
            Instant::now(),
        );
        let forward = transform_point(&frame.left.view_proj, [1.0, 0.0, 0.0]);
        assert!(forward[3] > 0.5, "w={}", forward[3]);
        let ndc_z = forward[2] / forward[3];
        assert!((0.0..=1.0).contains(&ndc_z), "z={ndc_z}");

        let backward = transform_point(&frame.left.view_proj, [-1.0, 0.0, 0.0]);
        assert!(backward[3] < 0.0, "w={}", backward[3]);
    }

    #[test]
    fn flat_2d_uses_identity_projection() {
        let frame = StereoFrame::mono_with_dimension(
            DimensionMode::Flat2D,
            HeadPose {
                position: [3.0, 1.0, -2.0],
                yaw: 1.0,
            },
            Projection::new(1.0, 1.5),
            0,
            Instant::now(),
        );
        let point = [0.5, -0.5, 0.25];
        let out = transform_point(&frame.left.view_proj, point);
        assert!((out[0] - point[0]).abs() < 1e-6);
        assert!((out[1] - point[1]).abs() < 1e-6);
        assert!((out[2] - point[2]).abs() < 1e-6);
        assert!((out[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn isometric_25d_uses_orthographic_projection() {
        let frame = StereoFrame::mono_with_dimension(
            DimensionMode::Isometric25D,
            HeadPose::default(),
            Projection::new(16.0 / 9.0, 1.5),
            0,
            Instant::now(),
        );
        // La perspectiva deja m[15] = 0; la ortográfica lo fija a 1.
        assert!((frame.left.view_proj[15] - 1.0).abs() < 1e-6);
        assert!(frame.left.view_proj.iter().all(|v| v.is_finite()));
    }

    /// `3.5d` es la cámara del `2.5d` con perspectiva parcial: con `blend = 0`
    /// la matriz tiene que ser *exactamente* la ortográfica isométrica, y con
    /// `blend = 1` una perspectiva del mismo encuadre sin que el cuadro salte.
    #[test]
    fn fractional_35d_blends_the_isometric_camera() {
        let pose = HeadPose {
            position: [4.0, 1.7, -3.0],
            yaw: 0.4,
        };
        let matrix_at = |blend: f32| {
            StereoFrame::mono_with_dimension(
                DimensionMode::Fractional35D,
                pose,
                Projection::new(16.0 / 9.0, 1.5).with_blend(blend),
                0,
                Instant::now(),
            )
            .left
            .view_proj
        };
        let iso = StereoFrame::mono_with_dimension(
            DimensionMode::Isometric25D,
            pose,
            Projection::new(16.0 / 9.0, 1.5),
            0,
            Instant::now(),
        )
        .left
        .view_proj;

        for (i, value) in matrix_at(0.0).iter().enumerate() {
            assert!(
                (value - iso[i]).abs() < 1e-6,
                "blend 0 debería ser la ortográfica de 2.5d (dif en m[{i}])"
            );
        }
        let full = matrix_at(1.0);
        // La firma de la perspectiva está en la fila w de la proyección: mezclar
        // con la ortográfica (fila w = 0,0,0,1) deja m[11] ≈ V[2][2] ≠ 0.
        assert!(
            full[11] > 0.5,
            "blend 1 no hereda la w de la perspectiva: m[11]={}",
            full[11]
        );
        assert!(
            iso[11].abs() < 1e-6,
            "la ortográfica de 2.5d no debería tener m[11]: {}",
            iso[11]
        );

        // El centro del encuadre isométrico no se mueve en todo el espectro.
        let focus = [pose.position[0], pose.position[1], pose.position[2] + 8.0];
        for blend in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let m = matrix_at(blend);
            assert!(m.iter().all(|v| v.is_finite()), "{blend} no finita");
            let p = transform_point(&m, focus);
            assert!(
                (p[0] / p[3]).abs() < 1e-3 && (p[1] / p[3]).abs() < 1e-3,
                "blend {blend}: el foco se salió del centro: {p:?}"
            );
        }

        // La perspectiva es real: más allá del foco, el punto se encoge al
        // subir el blend, y la ortográfica lo deja de tamaño constante.
        let far = [
            pose.position[0] + 6.0,
            pose.position[1],
            pose.position[2] + 40.0,
        ];
        let width_at = |blend: f32| {
            let p = transform_point(&matrix_at(blend), far);
            (p[0] / p[3]).abs()
        };
        assert!(
            width_at(1.0) < width_at(0.0) * 0.75,
            "sin perspectiva: {} vs {}",
            width_at(1.0),
            width_at(0.0)
        );
        assert!(width_at(0.5) < width_at(0.0));

        // Un blend corrupto no rompe la matriz: cae al punto medio.
        assert_eq!(matrix_at(f32::NAN), matrix_at(0.5));
    }

    #[test]
    fn isometric_camera_follows_the_local_offset() {
        // La cámara iso debe mirar al *offset* del jugador, no al origen global:
        // si mirara al origen, el punto central de un jugador alejado saldría
        // del cuadro ortográfico de ±11 m.
        let center_of_view = |focus: [f32; 3]| {
            let frame = StereoFrame::mono_with_dimension(
                DimensionMode::Isometric25D,
                HeadPose {
                    position: focus,
                    yaw: 0.0,
                },
                Projection::new(16.0 / 9.0, 1.5),
                0,
                Instant::now(),
            );
            // Lo que la cámara isométrica tiene en el centro de la pantalla.
            let p = transform_point(&frame.left.view_proj, [focus[0], focus[1], focus[2] + 8.0]);
            [p[0] / p[3], p[1] / p[3]]
        };
        let home = center_of_view([0.0, 1.7, 0.0]);
        let far = center_of_view([900.0, 1.7, -700.0]);
        for ndc in [home, far] {
            assert!(
                ndc[0].abs() < 0.02 && ndc[1].abs() < 0.02,
                "el jugador quedó fuera de cuadro: {ndc:?}"
            );
        }
    }

    #[test]
    fn sector_depth_range_reaches_the_projection() {
        // `far` deja de ser una constante: un sector de 40 km debe poder
        // expresarse y debe acabar en la matriz.
        let depth = DepthRange::new(2.0, 40_000.0);
        depth.validate().expect("sector válido");
        let frame = StereoFrame::mono_with_dimension(
            DimensionMode::Spatial3D,
            HeadPose {
                position: [0.0, 1.7, 0.0],
                yaw: 0.0,
            },
            Projection {
                aspect: 16.0 / 9.0,
                fov_y_rad: 1.5,
                perspective_blend: 0.5,
                depth,
            },
            0,
            Instant::now(),
        );
        let m = frame.left.view_proj;
        // m[10]/m[14] codifican near/far: far_m / (far_m - near_m) ≈ 1.00005,
        // -near*far/(far-near) ≈ -2.0000.
        assert!((m[10] - 40_000.0 / 39_998.0).abs() < 1e-4, "m10={}", m[10]);
        assert!(
            (m[14] + 40_000.0 * 2.0 / 39_998.0).abs() < 1e-2,
            "m14={}",
            m[14]
        );

        // La niebla acompaña al sector, y con el alcance por defecto reproduce
        // el look histórico (10 m de inicio, 55 m de rampa).
        let home = DepthRange::default().fog_params();
        assert!((home[0] - 10.0).abs() < 1e-3 && (home[1] - 55.0).abs() < 1e-2);
        let big = DepthRange::new(2.0, 40_000.0).fog_params();
        assert!((big[0] - 40.0).abs() < 1e-3 && (big[1] - 220.0).abs() < 1e-2);
        assert!((big[2] - home[2]).abs() < 1e-6);
        // El frame baja la niebla ya resuelta: el renderer no vuelve a la config.
        assert_eq!(frame.fog, depth.fog_params());
    }

    #[test]
    fn absurd_depth_range_is_rejected_then_clamped() {
        assert!(DepthRange::new(0.0, 1_000.0).validate().is_err());
        assert!(DepthRange::new(1.0, 0.5).validate().is_err());
        assert!(DepthRange::new(1.0, 1e12).validate().is_err());
        assert!(DepthRange::new(f32::NAN, 1e3).validate().is_err());
        // La matriz no puede recibir NaN aunque el rango lo esté: se pliega a
        // valores utilizables antes de la proyección.
        let c = DepthRange::new(f32::NAN, f32::INFINITY).clamped();
        assert!(c.near_m.is_finite() && c.far_m.is_finite());
        assert!(c.far_m > c.near_m);
        let frame = StereoFrame::mono_with_dimension(
            DimensionMode::Spatial3D,
            HeadPose::default(),
            Projection {
                aspect: 1.0,
                fov_y_rad: 1.5,
                perspective_blend: 0.5,
                depth: DepthRange::new(f32::NAN, f32::INFINITY),
            },
            0,
            Instant::now(),
        );
        assert!(frame.left.view_proj.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn absolute_coordinates_collapse_ipd_but_local_offsets_do_not() {
        // El motivo del origen flotante, medido sobre la propia matriz. La
        // posición global del juego vive en `f64`; el camino viejo la pasaba a
        // `f32` y ahí la mitad de la IPD desaparece en el ULP (a 1e11 m el ULP
        // del `f32` es 8192 m). Con offset local, no.
        let global_m: [f64; 3] = [1e11, 1.7, 0.0];
        let absolute = StereoFrame::stereo(
            HeadPose {
                position: global_m.map(|v| v as f32),
                yaw: 0.0,
            },
            0.063,
            0.5,
            1.5,
            0,
            0,
        );
        assert_eq!(
            absolute.left.position[0], absolute.right.position[0],
            "el f32 absoluto ya no debería colapsar: revisa la convención"
        );

        let origin = crate::coords::WorldOrigin::new([global_m[0], 0.0, global_m[2]]);
        let local = origin.to_local(global_m).expect("cabe");
        assert_eq!(local, [0.0, 1.7, 0.0]);
        let relative = StereoFrame::stereo(
            HeadPose {
                position: local,
                yaw: 0.0,
            },
            0.063,
            0.5,
            1.5,
            0,
            0,
        );
        let dx = relative.right.position[0] - relative.left.position[0];
        assert!((dx - 0.063).abs() < 1e-6, "estéreo local colapsó: dx={dx}");
    }

    #[test]
    fn every_dimension_mode_produces_finite_matrices() {
        for mode in [
            DimensionMode::Flat2D,
            DimensionMode::Isometric25D,
            DimensionMode::Spatial3D,
            DimensionMode::Fractional35D,
            DimensionMode::Hyper4D,
            DimensionMode::Fractional45D,
            DimensionMode::Penteract5D,
        ] {
            let mono = StereoFrame::mono_with_dimension(
                mode,
                HeadPose {
                    position: [1.0, 1.7, -2.0],
                    yaw: 0.7,
                },
                Projection::new(16.0 / 9.0, 1.5),
                0,
                Instant::now(),
            );
            assert!(
                mono.left.view_proj.iter().all(|v| v.is_finite()),
                "{mode:?}"
            );
            let stereo = StereoFrame::stereo_with_dimension(
                mode,
                HeadPose::default(),
                0.063,
                Projection::new(0.5, 1.5),
                0,
                0,
            );
            assert!(
                stereo.right.view_proj.iter().all(|v| v.is_finite()),
                "{mode:?}"
            );
        }
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
    fn pose_predictor_wraps_yaw_across_pi() {
        let mut predictor = PosePredictor::new();
        predictor.update(
            HeadPose {
                position: [0.0; 3],
                yaw: 3.13,
            },
            1_000_000_000,
        );
        // Cruce de +π a -π: el delta real es ~0.02 rad, no ~2π.
        predictor.update(
            HeadPose {
                position: [0.0; 3],
                yaw: -3.13,
            },
            1_010_000_000,
        );

        let pred = predictor.predict(1_020_000_000);
        assert!(
            pred.yaw.abs() <= std::f32::consts::PI + 1e-4,
            "yaw={} fuera de [-π, π]",
            pred.yaw
        );
        // La tasa debe reflejar el giro corto (≈2.3 rad/s), no el salto de 2π.
        assert!((pred.yaw - -3.13_f32).abs() < 0.05, "yaw={}", pred.yaw);
    }

    #[test]
    fn pose_predictor_ignores_non_finite_pose() {
        let mut predictor = PosePredictor::new();
        predictor.update(
            HeadPose {
                position: [1.0, 1.7, 2.0],
                yaw: 0.5,
            },
            1_000_000_000,
        );
        predictor.update(
            HeadPose {
                position: [f32::NAN, 1.7, 2.0],
                yaw: f32::INFINITY,
            },
            1_010_000_000,
        );
        let pred = predictor.predict(1_020_000_000);
        assert!(pred.position.iter().all(|v| v.is_finite()));
        assert!(pred.yaw.is_finite());
        assert!((pred.position[0] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn pose_filter_yaw_stays_continuous_across_pi() {
        let mut filter = PoseFilter::new(1.0, 0.0);
        let dt = 0.011;
        let a = filter.filter(
            HeadPose {
                position: [0.0; 3],
                yaw: 3.13,
            },
            dt,
        );
        let b = filter.filter(
            HeadPose {
                position: [0.0; 3],
                yaw: -3.13,
            },
            dt,
        );
        // El filtro no debe barrer el yaw por 0 al cruzar ±π.
        assert!(
            wrap_angle(b.yaw - a.yaw).abs() < 0.1,
            "a={} b={}",
            a.yaw,
            b.yaw
        );
        assert!(b.yaw.abs() <= std::f32::consts::PI + 1e-4);
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
