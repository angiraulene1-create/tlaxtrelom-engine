// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Configuración del runtime, validada antes de crear la GPU.
//!
//! Regla de estabilidad: **ningún valor entra al frame loop sin validar**. Un
//! `sim_hz <= 0`, un `ipd_m` absurdo o una resolución cero deben fallar en el
//! arranque con un mensaje accionable, no como un `NaN` tres horas después.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};

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
                "--no-fulldive" => {
                    cfg.fulldive.enabled = false;
                    cfg.fulldive.mock = false;
                }
                "--json" => cfg.json = true,
                "--secure" => {
                    cfg.fulldive.mock = false;
                    cfg.gate.min_discrete_confidence = 0.75;
                }
                _ => bail!("argumento desconocido: {arg} (usá --help)"),
            }
        }

        cfg.validate()?;
        Ok(cfg)
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
Tlaxtrelom Engine — runtime VR/FullDive

USO:
    tlaxtrelom [OPCIONES]

OPCIONES:
    --windowed              ventana de escritorio (por defecto)
    --stereo                preview estéreo side-by-side (VR sin runtime)
    --headless, --selftest  simulación sin ventana ni GPU (CI)
    --sim-hz <Hz>           frecuencia de simulación [30, 480] (120)
    --fps <Hz>              tasa objetivo de presentación (120)
    --ipd-mm <mm>           distancia interpupilar (63)
    --render-scale <f>      escala de render [0.5, 2.0] (1.0)
    --ticks <n>             pasos de simulación en headless (600)
    --telemetry-every <n>   frames entre resúmenes de telemetría (120)
    --replay <archivo>      reproducir un log .tlxneuro determinista
    --no-fulldive           no abrir transporte BCI (modo seguro)
    --secure                exige confianza alta para acciones discretas
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
}
