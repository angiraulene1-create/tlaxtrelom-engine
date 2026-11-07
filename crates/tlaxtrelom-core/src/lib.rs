// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! # Tlaxtrelom Engine — runtime
//!
//! Núcleo del motor: frame loop determinista, render wgpu, sesión XR/estéreo,
//! pila FullDive (BCI) y puente FFI con las islas nativas.
//!
//! El binario `tlaxtrelom` es solo un *host* de referencia; la lógica vive en
//! esta librería para que sea testeable sin abrir una ventana ni una GPU.

#![doc(html_no_source)]

pub mod config;
pub mod fulldive;
pub mod gate;
pub mod geometry;
pub mod hal;
pub mod headless;
pub mod metrics;
pub mod renderer;
pub mod world;
pub mod xr;

pub use config::{Config, RunMode};
pub use fulldive::{
    BiometricVitals, BiotelemetryWatchdog, DetachmentStatus, FullDiveHandle,
    GradualDetachmentController, IntentFrame, SomatosensoryLimiter, SomatosensoryPacket,
};
pub use gate::{GateConfig, IntentGate};
pub use geometry::{GeometryBridge, GeometryStats};
pub use hal::HalInfo;
pub use headless::SelfTestReport;
pub use metrics::{FrameMetrics, FramePacer, MetricsSnapshot};
pub use xr::{
    ComfortVignette, GazeSample, HeadPose, OneEuroFilter, PoseFilter, PosePredictor, StereoFrame,
    XrConfig, XrMode, XrSession,
};

/// Versión semántica del runtime, embebida en binarios y telemetría.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Identificador estable del motor para logs, SBOM y cache en disco.
pub const ENGINE_ID: &str = "tlaxtrelom-engine";

/// Ejecuta una comprobación rápida sin GPU ni ventana.
///
/// Es el modo que usa CI (`tlaxtrelom --selftest`) y el que debe seguir
/// funcionando en cualquier máquina, incluso sin drivers gráficos.
pub fn selftest(config: &Config) -> anyhow::Result<headless::SelfTestReport> {
    headless::run(config)
}
