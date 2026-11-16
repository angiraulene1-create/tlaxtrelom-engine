// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Configuración del runtime, validada antes de crear la GPU.
//!
//! Regla de estabilidad: **ningún valor entra al frame loop sin validar**. Un
//! `sim_hz <= 0`, un `ipd_m` absurdo o una resolución cero deben fallar en el
//! arranque con un mensaje accionable, no como un `NaN` tres horas después.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::dimension::{DimensionConfig, DimensionMode};
use crate::fulldive::Config as FullDiveConfig;
use crate::gate::GateConfig;
use crate::xr::XrConfig;

/// Modo de ejecución del host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Ventana de escritorio con render mono.
    Windowed,
    /// Ventana con vista estéreo side-by-side (preview de VR sin runtime XR).
    StereoPreview,
    /// Sin ventana ni GPU: simulación + FullDive + métricas (CI y servidores).
    Headless,
}

impl RunMode {
    pub fn is_headless(self) -> bool {
        matches!(self, Self::Headless)
    }

    pub fn is_stereo(self) -> bool {
        matches!(self, Self::StereoPreview)
    }
}

/// Configuración completa del motor. `Clone` porque el host puede recrear la
/// superficie (p. ej. al recuperarse de un `SurfaceError::Lost`) sin releer el
/// entorno.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: RunMode,
    /// Frecuencia autoritativa de simulación (Hz). Por defecto 120.
    pub sim_hz: f64,
    /// Tope de pasos por frame para evitar la espiral de la muerte.
    pub max_sim_steps: u32,
    /// `dt` máximo aceptado tras un stall (breakpoint, suspensión, resize).
    pub max_frame_dt: Duration,
    /// Tasa objetivo de presentación. 0 = sin límite (solo diagnóstico).
    pub target_fps: u32,
    /// Cada cuántos frames se emite telemetría agregada por stderr.
    pub telemetry_every: u64,
    /// Emitir el selftest como JSON (CI/automatización).
    pub json: bool,
    /// Número de frames consecutivos con error antes de abortar limpiamente.
    pub max_consecutive_errors: u32,
    /// Pasos de simulación en modo headless/selftest.
    pub headless_ticks: u64,
    pub xr: XrConfig,
    pub gate: GateConfig,
    pub fulldive: FullDiveConfig,
    /// Modo de renderizado dimensional y parámetros de proyección N-D.
    pub dimension: DimensionConfig,
    /// Sample count solicitado de MSAA (1, 2 o 4). El runtime puede reducirlo
    /// si el adaptador o el formato de superficie no lo soportan.
    pub msaa: u32,
    /// Forzar un archivo de replay `.tlxneuro` en lugar del transporte BCI.
    pub replay: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: RunMode::Windowed,
            sim_hz: 120.0,
            max_sim_steps: 8,
            max_frame_dt: Duration::from_millis(100),
            target_fps: 120,
            telemetry_every: 120,
            json: false,
            max_consecutive_errors: 3,
            headless_ticks: 600,
            xr: XrConfig::default(),
            gate: GateConfig::default(),
            fulldive: FullDiveConfig::default(),
            dimension: DimensionConfig::default(),
            msaa: 1,
            replay: None,
        }
    }
}

impl Config {
    /// Lee la configuración de `std::env::args()`.
    pub fn from_env() -> Result<Self> {
        Self::parse(std::env::args().skip(1))
    }

    /// Parser sin dependencias externas. Deliberadamente estricto: un flag
    /// desconocido es un error, no un *silent ignore*.
    pub fn parse<I>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let mut cfg = Self::default();
        let mut it = args.into_iter();

        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--headless" | "--selftest" => cfg.mode = RunMode::Headless,
                "--stereo" => {
                    cfg.mode = RunMode::StereoPreview;
                    cfg.xr.mode = crate::xr::XrMode::StereoPreview;
                    cfg.xr.refresh_hz = 90;
                    cfg.target_fps = cfg.xr.refresh_hz;
                }
                "--windowed" => {
                    cfg.mode = RunMode::Windowed;
                    cfg.xr.mode = crate::xr::XrMode::Off;
                }
                "--sim-hz" => cfg.sim_hz = parse_value(&mut it, "--sim-hz")?,
                "--fps" => {
                    cfg.target_fps = parse_value(&mut it, "--fps")?;
                    cfg.xr.refresh_hz = cfg.target_fps.max(1);
                }
                "--ipd-mm" => {
                    let mm: f32 = parse_value(&mut it, "--ipd-mm")?;
                    cfg.xr.ipd_m = mm / 1000.0;
                }
                "--render-scale" => cfg.xr.render_scale = parse_value(&mut it, "--render-scale")?,
                "--sector" => {
                    let m: f32 = parse_value(&mut it, "--sector")?;
                    // El `near` escala con el alcance: el depth buffer reparte
                    // su resolución en [near, far] y un sector de 40 km con
                    // near de 5 cm concentra todo el detalle en el infinito.
                    let near = (m / 200_000.0).clamp(0.02, 200.0);
                    cfg.xr.depth = crate::xr::DepthRange::new(near, m);
                }
                "--ticks" => cfg.headless_ticks = parse_value(&mut it, "--ticks")?,
                "--telemetry-every" => {
                    cfg.telemetry_every = parse_value(&mut it, "--telemetry-every")?
                }
                "--replay" => {
                    let path: String = parse_value(&mut it, "--replay")?;
                    cfg.replay = Some(PathBuf::from(path));
                    cfg.fulldive.replay = cfg.replay.clone();
                    cfg.fulldive.mock = false;
                    cfg.fulldive.enabled = true;
                }
                "--fulldive" => cfg.fulldive.enabled = true,
                "--no-fulldive" => {
                    cfg.fulldive.enabled = false;
                    cfg.fulldive.mock = false;
                }
                "--dimension" => {
                    let raw: String = parse_value(&mut it, "--dimension")?;
                    let mode = DimensionMode::parse(&raw).with_context(|| {
                        format!(
                            "--dimension: modo inválido {raw:?} (2d, 2.5d, 3d, 3.5d, 4d, 4.5d, 5d)"
                        )
                    })?;
                    cfg.dimension.mode = mode;
                    cfg.xr.dimension = mode;
                }
                "--dimension-blend" => {
                    cfg.dimension.blend = parse_value(&mut it, "--dimension-blend")?;
                    // El blend del modo fraccionario también vive en la sesión
                    // XR (es la mezcla orto↔persp del 3.5d): viajan juntos.
                    cfg.xr.perspective_blend = cfg.dimension.blend;
                }
                "--dimension-focal" => {
                    cfg.dimension.focal_distance = parse_value(&mut it, "--dimension-focal")?
                }
                "--msaa" => cfg.msaa = parse_value(&mut it, "--msaa")?,
                "--json" => cfg.json = true,
                "--secure" => {
                    cfg.fulldive.mock = false;
                    cfg.gate.min_discrete_confidence = 0.75;
                }
                _ => bail!("argumento desconocido: {arg} (usá --help)"),
            }
        }

        cfg.validate_displayed()?;
        Ok(cfg)
    }

    /// Valida y deja el detalle visible en `Display`.
    ///
    /// `anyhow::Error` sólo imprime el mensaje raíz en su `Display`; sin esto,
    /// un `--dimension-blend 2.0` malo terminaba en un críptico
    /// "configuración dimensional inválida" sin nombrar el flag. El `Debug`
    /// (lo que imprime `main`) sí despliega la cadena.
    fn validate_displayed(&self) -> Result<()> {
        self.validate()
            .map_err(|err| anyhow::anyhow!(flatten_error_chain(&err)))
    }

    /// Valida rangos físicos y de seguridad. Debe llamarse siempre antes de
    /// construir el `Engine`.
    pub fn validate(&self) -> Result<()> {
        if !(30.0..=480.0).contains(&self.sim_hz) {
            bail!("sim_hz fuera de rango [30, 480]: {}", self.sim_hz);
        }
        if self.max_sim_steps == 0 || self.max_sim_steps > 32 {
            bail!(
                "max_sim_steps fuera de rango [1, 32]: {}",
                self.max_sim_steps
            );
        }
        if self.max_frame_dt.is_zero() || self.max_frame_dt > Duration::from_secs(1) {
            bail!("max_frame_dt fuera de rango (0, 1s]");
        }
        if self.target_fps > 480 {
            bail!("target_fps fuera de rango [0, 480]: {}", self.target_fps);
        }
        if self.headless_ticks > 10_000_000 {
            bail!("headless_ticks demasiado alto: {}", self.headless_ticks);
        }
        if !matches!(self.msaa, 1 | 2 | 4) {
            bail!("msaa fuera de {{1, 2, 4}}: {}", self.msaa);
        }
        self.dimension
            .validate()
            .context("configuración dimensional inválida")?;
        if self.dimension.mode != self.xr.dimension {
            bail!(
                "modo dimensional inconsistente: config={:?} xr={:?}",
                self.dimension.mode,
                self.xr.dimension
            );
        }
        if (self.xr.perspective_blend - self.dimension.blend).abs() > 1e-6 {
            bail!(
                "mezcla dimensional inconsistente: config={} xr={}",
                self.dimension.blend,
                self.xr.perspective_blend
            );
        }
        self.xr.validate().context("configuración XR inválida")?;
        self.gate
            .validate()
            .context("configuración de gate inválida")?;
        self.fulldive
            .validate()
            .context("configuración FullDive inválida")?;

        if self.replay.is_some() && self.fulldive.replay.is_none() {
            bail!("replay indicado en Config pero no propagado a FullDive");
        }
        Ok(())
    }

    /// Periodo objetivo de presentación; `None` si el host es uncapped.
    pub fn frame_period(&self) -> Option<Duration> {
        (self.target_fps > 0).then(|| Duration::from_secs_f64(1.0 / self.target_fps as f64))
    }

    /// Presupuesto de CPU por frame en microsegundos.
    pub fn frame_budget_us(&self) -> u32 {
        self.frame_period()
            .map(|p| p.as_micros().min(u32::MAX as u128) as u32)
            .unwrap_or(0)
    }

    /// Aplica la configuración XR al frame rate efectivo.
    pub fn apply_xr_to_fps(&mut self) {
        if self.xr.mode != crate::xr::XrMode::Off && self.xr.refresh_hz > 0 {
            self.target_fps = self.xr.refresh_hz;
        }
    }
}

/// Aplana la cadena de causas de un `anyhow::Error` en un solo mensaje.
fn flatten_error_chain(err: &anyhow::Error) -> String {
    let mut out = String::new();
    for (depth, cause) in err.chain().enumerate() {
        if depth > 0 {
            out.push_str(": ");
        }
        out.push_str(&cause.to_string());
    }
    out
}

fn parse_value<T>(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = it
        .next()
        .with_context(|| format!("{flag} requiere un valor"))?;
    raw.parse::<T>()
        .map_err(|e| anyhow::anyhow!("{flag}: valor inválido {raw:?}: {e}"))
}

/// Texto de ayuda; vive en el runtime para que `--help` no dependa del host.
pub const HELP: &str = "\
Tlaxtrelom Engine — Motor gráfico determinista de alta fidelidad (2D, 2.5D, 3D, 3.5D, 4D, 4.5D, 5D)

USO:
    tlaxtrelom [OPCIONES]

OPCIONES:
    --windowed              ventana de escritorio estándar (por defecto)
    --stereo                preview estéreo side-by-side (VR sin runtime)
    --headless, --selftest  simulación sin ventana ni GPU (CI / diagnóstico)
    --sim-hz <Hz>           frecuencia de simulación [30, 480] (120)
    --fps <Hz>              tasa objetivo de presentación (120)
    --ipd-mm <mm>           distancia interpupilar (63)
    --render-scale <f>      escala de render [0.5, 2.0] (1.0)
    --sector <m>            alcance del sector en metros [1, 1e8] (10000)
    --ticks <n>             pasos de simulación en headless (600)
    --telemetry-every <n>   frames entre resúmenes de telemetría (120)
    --replay <archivo>      reproducir un log .tlxneuro determinista
    --dimension <modo>      2d, 2.5d, 3d (defecto), 3.5d, 4d, 4.5d o 5d
    --dimension-blend <f>   mezcla orto↔persp en 3.5d o morph 3D↔4D en 4.5d [0, 1] (0.5)
    --dimension-focal <f>   distancia focal N-D [0.1, 1000] (6.0)
    --msaa <n>              antialiasing 1, 2 o 4 muestras (1)
    --fulldive              activa el complemento experimental FullDive en segundo plano (opt-in)
    --no-fulldive           fuerza FullDive apagado (modo motor gráfico normal por defecto)
    --secure                exige confianza alta (0.75) para acciones discretas
    --json                  selftest en JSON (para CI)
    -h, --help              muestra esta ayuda
    -V, --version           muestra la versión
";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().expect("defaults válidos");
    }

    /// FullDive es un complemento experimental: apagado por defecto para que el
    /// motor arranque como un motor gráfico normal, y activable explícitamente.
    #[test]
    fn fulldive_is_off_by_default_and_opt_in() {
        assert!(!Config::default().fulldive.enabled);
        assert!(!Config::parse(args(&[])).unwrap().fulldive.enabled);

        let on = Config::parse(args(&["--fulldive"])).unwrap();
        assert!(on.fulldive.enabled);
        assert!(on.fulldive.mock, "sin --secure el transporte es mock");

        // El parser es secuencial: el último flag de la pareja gana.
        let off = Config::parse(args(&["--fulldive", "--no-fulldive"])).unwrap();
        assert!(!off.fulldive.enabled);
        assert!(!off.fulldive.mock);

        // `--secure` no abre el BCI por sí solo: exige confianza y hardware
        // real, pero seguir sin complemento es decisión de `--fulldive`.
        let secure = Config::parse(args(&["--secure"])).unwrap();
        assert!(!secure.fulldive.enabled);
        assert!(!secure.fulldive.mock);
    }

    #[test]
    fn dimension_35d_parses_and_syncs_the_camera_blend() {
        let cfg = Config::parse(args(&["--dimension", "3.5d", "--dimension-blend", "0.25"]))
            .expect("3.5d válido");
        assert_eq!(cfg.dimension.mode, DimensionMode::Fractional35D);
        assert_eq!(cfg.xr.dimension, DimensionMode::Fractional35D);
        assert!((cfg.xr.perspective_blend - 0.25).abs() < 1e-6);

        // El blend viaja en dos lados y tienen que coincidir.
        let mut roto = cfg.clone();
        roto.xr.perspective_blend = 0.9;
        let err = roto.validate().unwrap_err();
        assert!(err.to_string().contains("mezcla dimensional"), "{err}");

        let err = Config::parse(args(&["--dimension", "3.75d"])).unwrap_err();
        assert!(err.to_string().contains("3.5d"), "{err}");
    }

    #[test]
    fn rejects_bad_sim_hz() {
        let err = Config::parse(args(&["--sim-hz", "0"])).unwrap_err();
        assert!(err.to_string().contains("sim_hz"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = Config::parse(args(&["--wat"])).unwrap_err();
        assert!(err.to_string().contains("desconocido"));
    }

    #[test]
    fn stereo_sets_xr_mode_and_fps() {
        let cfg = Config::parse(args(&["--stereo", "--fps", "120"])).unwrap();
        assert_eq!(cfg.mode, RunMode::StereoPreview);
        assert_eq!(cfg.xr.mode, crate::xr::XrMode::StereoPreview);
        assert_eq!(cfg.target_fps, 120);
    }

    #[test]
    fn ipd_is_converted_to_metres() {
        let cfg = Config::parse(args(&["--ipd-mm", "64"])).unwrap();
        assert!((cfg.xr.ipd_m - 0.064).abs() < 1e-6);
    }

    #[test]
    fn frame_budget_matches_rate() {
        let cfg = Config::default();
        assert_eq!(cfg.frame_budget_us(), 8_333);
    }

    #[test]
    fn dimension_flag_parses_and_syncs_xr() {
        let cfg = Config::parse(args(&["--dimension", "4.5d"])).unwrap();
        assert_eq!(cfg.dimension.mode, DimensionMode::Fractional45D);
        assert_eq!(cfg.xr.dimension, DimensionMode::Fractional45D);
    }

    #[test]
    fn rejects_unknown_dimension() {
        let err = Config::parse(args(&["--dimension", "7d"])).unwrap_err();
        assert!(err.to_string().contains("dimension"));
    }

    #[test]
    fn rejects_invalid_msaa() {
        let err = Config::parse(args(&["--msaa", "3"])).unwrap_err();
        assert!(err.to_string().contains("msaa"));
    }

    #[test]
    fn rejects_invalid_dimension_blend() {
        let err = Config::parse(args(&["--dimension-blend", "2.0"])).unwrap_err();
        assert!(err.to_string().contains("blend"));
    }

    /// `Display` es lo que ve una herramienta que captura `err.to_string()`:
    /// si sólo muestra el contexto raíz, el usuario no sabe qué flag arreglar.
    #[test]
    fn parse_errors_name_the_failing_parameter_in_display() {
        for (flag, value, expected) in [
            ("--render-scale", "9.0", "render_scale"),
            ("--ipd-mm", "500", "ipd_m"),
            ("--dimension-focal", "0.0", "focal_distance"),
            ("--dimension-blend", "2.0", "dimension_blend"),
        ] {
            let err = Config::parse(args(&[flag, value])).unwrap_err();
            let display = err.to_string();
            assert!(
                display.contains(expected),
                "{flag} {value}: Display={display:?} debería mencionar {expected:?}"
            );
        }
    }

    #[test]
    fn sector_flag_sets_the_depth_range() {
        let cfg = Config::parse(args(&["--sector", "40000"])).expect("sector válido");
        assert!((cfg.xr.depth.far_m - 40_000.0).abs() < 1.0);
        assert!((cfg.xr.depth.near_m - 0.2).abs() < 1e-3);
        assert!(cfg.xr.depth.fog_params()[0] > 30.0);

        // El alcance por defecto coincide con el valor histórico del motor.
        let same = Config::parse(args(&["--sector", "10000"])).expect("default explícito");
        assert!((same.xr.depth.near_m - 0.05).abs() < 1e-6);
        assert!((same.xr.depth.far_m - 10_000.0).abs() < 1.0);

        let err = Config::parse(args(&["--sector", "9e12"])).unwrap_err();
        assert!(
            format!("{err:#}").contains("far_m"),
            "el error debería nombrar el parámetro: {err:#}"
        );
    }

    #[test]
    fn rejects_inconsistent_dimension_modes() {
        let mut cfg = Config::default();
        cfg.dimension.mode = DimensionMode::Hyper4D;
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("inconsistente"));
    }
}
