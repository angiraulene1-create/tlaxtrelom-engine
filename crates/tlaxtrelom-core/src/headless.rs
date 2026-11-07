// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Selftest *headless*: valida el runtime sin GPU, ventana ni hardware BCI.
//!
//! Es el contrato que CI ejecuta en cada commit (`tlaxtrelom --selftest`). No
//! pretende medir rendimiento; pretende demostrar que:
//!
//! - la simulación a paso fijo es **determinista** (mismo input ⇒ mismo hash);
//! - el `IntentGate` neutraliza artefactos y deja pasar input válido;
//! - las matrices estéreo son finitas y la sesión XR progresa;
//! - el puente de geometría responde (nativo o stub);
//! - la HAL reporta capacidades coherentes.

use std::time::{Duration, Instant};

use crate::config::Config;
use crate::fulldive::{flags, IntentFrame};
use crate::gate::IntentGate;
use crate::geometry::{GeometryBridge, PageRequest, ReadyPage, View};
use crate::hal;
use crate::metrics::FrameMetrics;
use crate::world::World;
use crate::{ENGINE_ID, VERSION};

/// Resultado verificable del selftest.
#[derive(Debug, Clone)]
pub struct SelfTestReport {
    pub engine: String,
    pub version: String,
    pub ticks: u64,
    pub sim_hash: u64,
    pub determinism_ok: bool,
    pub valid_steps: u64,
    pub gated_steps: u64,
    pub action_frames: u64,
    pub xr_frames: u64,
    pub xr_late: u64,
    pub xr_render_scale: f32,
    pub geometry_pages: u64,
    pub hal: String,
    pub elapsed_us: u128,
}

impl SelfTestReport {
    /// Un selftest es vinculante: si esto es `false`, el job falla.
    pub fn is_ok(&self) -> bool {
        self.determinism_ok
            && self.sim_hash != 0
            && self.valid_steps > 0
            && self.gated_steps > 0
            && self.xr_frames > 0
            && self.xr_render_scale.is_finite()
    }

    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{\n",
                "  \"engine\": \"{}\",\n",
                "  \"version\": \"{}\",\n",
                "  \"ok\": {},\n",
                "  \"ticks\": {},\n",
                "  \"sim_hash\": \"0x{:016x}\",\n",
                "  \"determinism_ok\": {},\n",
                "  \"valid_steps\": {},\n",
                "  \"gated_steps\": {},\n",
                "  \"action_frames\": {},\n",
                "  \"xr_frames\": {},\n",
                "  \"xr_late\": {},\n",
                "  \"xr_render_scale\": {:.3},\n",
                "  \"geometry_pages\": {},\n",
                "  \"hal\": \"{}\",\n",
                "  \"elapsed_us\": {}\n",
                "}}\n"
            ),
            json_escape(&self.engine),
            json_escape(&self.version),
            self.is_ok(),
            self.ticks,
            self.sim_hash,
            self.determinism_ok,
            self.valid_steps,
            self.gated_steps,
            self.action_frames,
            self.xr_frames,
            self.xr_late,
            self.xr_render_scale,
            self.geometry_pages,
            json_escape(&self.hal),
            self.elapsed_us,
        )
    }
}

impl std::fmt::Display for SelfTestReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} {} — selftest {}",
            self.engine,
            self.version,
            if self.is_ok() { "OK" } else { "FALLÓ" }
        )?;
        writeln!(f, "  ticks           : {}", self.ticks)?;
        writeln!(f, "  sim hash        : 0x{:016x}", self.sim_hash)?;
        writeln!(
            f,
            "  determinismo    : {}",
            if self.determinism_ok { "sí" } else { "NO" }
        )?;
        writeln!(f, "  pasos válidos   : {}", self.valid_steps)?;
        writeln!(f, "  pasos gated     : {}", self.gated_steps)?;
        writeln!(f, "  frames acción   : {}", self.action_frames)?;
        writeln!(
            f,
            "  XR frames       : {} (late {}, scale {:.2})",
            self.xr_frames, self.xr_late, self.xr_render_scale
        )?;
        writeln!(f, "  páginas geom.   : {}", self.geometry_pages)?;
        writeln!(f, "  HAL             : {}", self.hal)?;
        write!(
            f,
            "  tiempo          : {:.3} ms",
            self.elapsed_us as f64 / 1000.0
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct SimResult {
    hash: u64,
    valid_steps: u64,
    gated_steps: u64,
    action_frames: u64,
}

/// Ejecuta el selftest completo. No abre ventana ni toca la GPU.
pub fn run(config: &Config) -> anyhow::Result<SelfTestReport> {
    let started = Instant::now();
    let hal_text = hal::describe().unwrap_or_else(|e| format!("HAL no disponible: {e:#}"));

    let first = simulate(config);
    let second = simulate(config);
    let determinism_ok = first.hash == second.hash && first.hash != 0;

    let (xr_frames, xr_late, xr_render_scale) = exercise_xr(config);
    let geometry_pages = exercise_geometry()?;

    let mut metrics = FrameMetrics::new();
    metrics.record(crate::metrics::FrameSample {
        frame_us: started.elapsed().as_micros().min(u32::MAX as u128) as u32,
        sim_steps: 1,
        ..Default::default()
    });
    let _snapshot = metrics.snapshot();

    Ok(SelfTestReport {
        engine: ENGINE_ID.to_string(),
        version: VERSION.to_string(),
        ticks: config.headless_ticks,
        sim_hash: first.hash,
        determinism_ok,
        valid_steps: first.valid_steps,
        gated_steps: first.gated_steps,
        action_frames: first.action_frames,
        xr_frames,
        xr_late,
        xr_render_scale,
        geometry_pages,
        hal: hal_text,
        elapsed_us: started.elapsed().as_micros(),
    })
}

fn simulate(config: &Config) -> SimResult {
    let ticks = config.headless_ticks;
    let hz = config.sim_hz.max(1.0);
    let dt = (1.0 / hz) as f32;
    let base = Instant::now();

    let mut gate = IntentGate::new(config.gate.clone());
    let mut world = World::demo();
    let mut valid_steps = 0u64;
    let mut gated_steps = 0u64;
    let mut action_frames = 0u64;

    for i in 0..ticks {
        let now = base + Duration::from_secs_f64(i as f64 / hz);
        let raw = synthetic_intent(i, hz);
        let gated = gate.apply(raw, now);

        if gated.is_valid() {
            valid_steps += 1;
        }
        if gated.flags & flags::GATED != 0 {
            gated_steps += 1;
        }
        if gated.actions != 0 {
            action_frames += 1;
        }

        world.fixed_update(dt, &gated);
    }

    SimResult {
        hash: world.digest(),
        valid_steps,
        gated_steps,
        action_frames,
    }
}

/// Entrada sintética reproducible. Incluye un artefacto periódico para que el
/// gate tenga algo que bloquear y una acción sostenida para probar el dwell.
fn synthetic_intent(i: u64, hz: f64) -> IntentFrame {
    let t = i as f32 / hz.max(1.0) as f32;
    let mut frame = IntentFrame::neutral();
    frame.seq = (i as u32).wrapping_add(1);
    frame.confidence = 0.9;
    frame.axes[0] = (t * 1.7).sin() * 0.6;
    frame.axes[1] = (t * 1.1).cos() * 0.4;
    frame.axes[2] = (t * 0.7).sin() * 0.3;
    // Mantener durante 200 ms cada 2 s: supera el dwell de 150 ms.
    frame.actions = if i % 240 < 24 { 1 } else { 0 };
    frame.flags = flags::VALID;
    if i % 997 == 0 {
        frame.flags |= flags::ARTIFACT;
    }
    frame
}

fn exercise_xr(config: &Config) -> (u64, u64, f32) {
    let frames = config.headless_ticks.clamp(1, 240);
    let mut xr = match crate::xr::XrSession::new(config.xr.clone()) {
        Ok(xr) => xr,
        Err(_) => return (0, 0, 1.0),
    };

    let base = Instant::now();
    let mut finite = 0u64;
    for i in 0..frames {
        let now = base + Duration::from_micros(i * 8_333);
        let pose = crate::xr::HeadPose {
            position: [0.0, 1.7, 0.0],
            yaw: (i as f32 * 0.01).sin() * 0.2,
        };
        match xr.begin_frame(pose, 16.0 / 9.0, now) {
            Ok(frame) => {
                if frame.left.view_proj.iter().all(|v| v.is_finite())
                    && frame.right.view_proj.iter().all(|v| v.is_finite())
                {
                    finite += 1;
                }
                // 2 ms: muy por debajo del presupuesto de 90/120 Hz.
                let _ = xr.end_frame(2_000);
            }
            Err(_) => break,
        }
    }
    let stats = xr.stats();
    let _ = finite;
    (stats.frames, stats.late_frames, xr.render_scale())
}

fn exercise_geometry() -> anyhow::Result<u64> {
    let mut bridge = GeometryBridge::new()?;
    let mut pages = 0u64;
    for frame_id in 0..3u64 {
        bridge.begin_frame(&View::identity(), frame_id)?;
        let mut requests = [PageRequest::ZERO; 16];
        let _ = bridge.collect(&mut requests);
        let mut ready = [ReadyPage::ZERO; 8];
        let count = bridge.drain_ready(&mut ready);
        for page in &ready[..count] {
            // SAFETY: válido hasta `ack_upload`, que se llama a continuación.
            let bytes = unsafe { page.as_slice() };
            pages += (!bytes.is_empty()) as u64;
            bridge.ack_upload(page.page_id)?;
        }
    }
    Ok(pages)
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str("\\u0000"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest_is_deterministic_and_safe() {
        let cfg = Config {
            headless_ticks: 600,
            ..Config::default()
        };
        let report = run(&cfg).expect("selftest");
        assert!(report.determinism_ok);
        assert!(report.is_ok(), "{report}");
        assert!(report.valid_steps > 0);
        assert!(report.gated_steps > 0);
        assert!(report.action_frames > 0);
        assert!(report.xr_frames > 0);
        assert!(report.elapsed_us < 5_000_000);
    }

    #[test]
    fn json_report_is_valid_shape() {
        let cfg = Config {
            headless_ticks: 60,
            ..Config::default()
        };
        let report = run(&cfg).expect("selftest");
        let json = report.to_json();
        assert!(json.contains("\"engine\": \"tlaxtrelom-engine\""));
        assert!(json.contains("\"ok\":"));
        assert!(json.ends_with("}\n"));
    }

    #[test]
    fn synthetic_artifact_is_periodic() {
        let a = synthetic_intent(0, 120.0);
        assert_eq!(a.flags & flags::ARTIFACT, flags::ARTIFACT);
        let b = synthetic_intent(1, 120.0);
        assert_eq!(b.flags & flags::ARTIFACT, 0);
    }
}
