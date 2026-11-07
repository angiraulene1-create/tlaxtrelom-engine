// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! # Tlaxtrelom Engine — host de referencia
//!
//! Este binario es el *host* del motor: posee el bucle principal, el device
//! gráfico (`wgpu` ⇒ Vulkan/DX12/Metal), la sesión XR/estéreo y el hilo de
//! tiempo real del canal FullDive.
//!
//! El diseño busca que **ningún subsistema de otro lenguaje esté en el camino
//! caliente por objeto**:
//!
//! - C++ hace streaming de geometría por lotes (una vez por frame).
//! - C11 aporta la HAL de plataforma (capacidades, RT, hash).
//! - Python/Lua/TypeScript viven en herramientas, editor y CI.
//!
//! Modos:
//!
//! ```text
//! tlaxtrelom                 ventana mono a 120 Hz
//! tlaxtrelom --stereo        preview estéreo side-by-side
//! tlaxtrelom --headless      selftest determinista sin GPU
//! tlaxtrelom --probe         enumera adaptadores wgpu
//! ```
//!
//! Recuperación: `SurfaceError::Lost/Outdated` reconfigura y continúa;
//! `Timeout` se reintenta; `OutOfMemory` aborta limpio. La simulación nunca
//! avanza más de `max_sim_steps` por frame (sin espiral de la muerte).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tlaxtrelom_core::config::HELP;
use tlaxtrelom_core::fulldive::{flags, FullDiveHandle};
use tlaxtrelom_core::gate::IntentGate;
use tlaxtrelom_core::geometry::{GeometryBridge, PageRequest, ReadyPage, View};
use tlaxtrelom_core::metrics::{FrameMetrics, FramePacer, FrameSample};
use tlaxtrelom_core::renderer::{self, Gpu, Renderer};
use tlaxtrelom_core::world::World;
use tlaxtrelom_core::xr::XrSession;
use tlaxtrelom_core::{selftest, Config, RunMode, VERSION};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

#[derive(Debug, Default)]
struct InputState {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    turn_left: bool,
    turn_right: bool,
    sprint: bool,
    action: bool,
    mouse_yaw_delta: f32,
    mouse_down: bool,
    last_cursor: Option<(f64, f64)>,
    seq: u32,
}

struct Engine {
    cfg: Config,
    window: Arc<Window>,
    gpu: Gpu,
    renderer: Renderer,
    world: World,
    fulldive: FullDiveHandle,
    gate: IntentGate,
    xr: XrSession,
    geometry: GeometryBridge,
    pacer: Option<FramePacer>,
    metrics: FrameMetrics,
    input: InputState,
    last_frame: Instant,
    sim_accum: Duration,
    elapsed: Duration,
    frame_id: u64,
    consecutive_errors: u32,
    paused: bool,
    fullscreen: bool,
}

impl Engine {
    fn new(window: Arc<Window>, cfg: Config) -> Result<Self> {
        let gpu = pollster::block_on(Gpu::new(window.clone(), &cfg))?;
        let renderer = Renderer::new(&gpu.device, gpu.config.format);

        // FullDive: mock en desarrollo, replay en tests, hardware en producción.
        let fulldive = FullDiveHandle::spawn(cfg.fulldive.clone())
            .context("no se pudo iniciar el subsistema FullDive")?;
        let gate = IntentGate::new(cfg.gate.clone());
        let xr = XrSession::new(cfg.xr.clone()).context("configuración XR inválida")?;
        let mut geometry =
            GeometryBridge::new().context("no se pudo cargar tlaxtrelom-geometry (C++)")?;

        // Intentar registrar asset de geometría virtual si existe
        for asset_path in [
            "assets/demo_cluster.tlxg",
            "/tmp/tlaxtrelom-geo/demo_cluster.tlxg",
        ] {
            if std::path::Path::new(asset_path).exists() {
                if let Ok(c_path) = std::ffi::CString::new(asset_path) {
                    if let Err(e) = geometry.register_asset(1, &c_path) {
                        eprintln!("[tlaxtrelom] aviso: no se pudo registrar {asset_path}: {e}");
                    } else {
                        eprintln!(
                            "[tlaxtrelom] asset de geometría virtual registrado: {asset_path}"
                        );
                        break;
                    }
                }
            }
        }

        let now = Instant::now();
        let pacer = (cfg.target_fps > 0).then(|| FramePacer::new(cfg.target_fps, now));

        Ok(Self {
            cfg,
            window,
            gpu,
            renderer,
            world: World::demo(),
            fulldive,
            gate,
            xr,
            geometry,
            pacer,
            metrics: FrameMetrics::new(),
            input: InputState::default(),
            last_frame: now,
            sim_accum: Duration::ZERO,
            elapsed: Duration::ZERO,
            frame_id: 0,
            consecutive_errors: 0,
            paused: false,
            fullscreen: false,
        })
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pacer.as_ref().map(FramePacer::next_deadline)
    }

    fn tick(&mut self) -> Result<()> {
        let now = Instant::now();
        if self.paused {
            self.last_frame = now;
            return Ok(());
        }

        let raw_dt = now.saturating_duration_since(self.last_frame);
        self.last_frame = now;
        let dt = raw_dt.min(self.cfg.max_frame_dt);
        self.elapsed += dt;
        let frame_started = Instant::now();

        // ── 1. Input: teclado / ratón + lectura wait-free del último intent BCI ────
        let mut intent = self.fulldive.snapshot(now);
        while let Some(ev) = self.fulldive.next_event() {
            self.world.on_neural_event(ev);
        }

        // Procesar controles interactivos de teclado y ratón
        let speed_mult = if self.input.sprint { 2.0 } else { 1.0 };
        let mut kb_strafe = 0.0f32;
        let mut kb_fwd = 0.0f32;
        let mut kb_turn = 0.0f32;

        if self.input.forward {
            kb_fwd += 1.0;
        }
        if self.input.backward {
            kb_fwd -= 1.0;
        }
        if self.input.right {
            kb_strafe += 1.0;
        }
        if self.input.left {
            kb_strafe -= 1.0;
        }
        if self.input.turn_right {
            kb_turn += 1.0;
        }
        if self.input.turn_left {
            kb_turn -= 1.0;
        }

        kb_turn += self.input.mouse_yaw_delta;
        self.input.mouse_yaw_delta = 0.0;

        let has_interactive =
            kb_fwd != 0.0 || kb_strafe != 0.0 || kb_turn != 0.0 || self.input.action;

        if has_interactive {
            self.input.seq = self.input.seq.wrapping_add(1);

            // Orientar movimiento relativo a la orientación actual de la cámara (yaw)
            let (sin_yaw, cos_yaw) = self.world.player.yaw.sin_cos();
            let move_x = (kb_strafe * cos_yaw + kb_fwd * sin_yaw) * speed_mult;
            let move_z = (-kb_strafe * sin_yaw + kb_fwd * cos_yaw) * speed_mult;

            if !intent.is_valid() {
                intent.seq = self.input.seq;
                intent.t_ns = self.elapsed.as_nanos().min(u64::MAX as u128) as u64;
                intent.flags = flags::VALID;
                intent.confidence = 1.0;
                intent.axes[0] = move_x.clamp(-1.0, 1.0);
                intent.axes[1] = move_z.clamp(-1.0, 1.0);
                intent.axes[2] = kb_turn.clamp(-1.0, 1.0);
                if self.input.action {
                    intent.actions = 1;
                }
            } else {
                intent.axes[0] = (intent.axes[0] + move_x).clamp(-1.0, 1.0);
                intent.axes[1] = (intent.axes[1] + move_z).clamp(-1.0, 1.0);
                intent.axes[2] = (intent.axes[2] + kb_turn).clamp(-1.0, 1.0);
                if self.input.action {
                    intent.actions |= 1;
                }
            }
        }

        // Predicción: compensa la latencia fisiológica del decoder. El horizonte
        // (75 ms) es configurable por juego; se marca `PREDICTED` para que el
        // gameplay pueda desactivarla en acciones críticas.
        if intent.is_valid() {
            let horizon = 0.075;
            for i in 0..4 {
                intent.axes[i] = (intent.axes[i] + intent.axes[i + 4] * horizon).clamp(-1.0, 1.0);
            }
            intent.flags |= flags::PREDICTED;
        }

        // El gate es la última línea de seguridad antes del gameplay.
        intent = self.gate.apply(intent, now);

        // ── 2. Simulación a paso fijo ────────────────────────────────────────
        let step = Duration::from_secs_f64(1.0 / self.cfg.sim_hz);
        self.sim_accum += dt;
        let mut steps = 0u32;
        while self.sim_accum >= step && steps < self.cfg.max_sim_steps {
            self.world.fixed_update(step.as_secs_f32(), &intent);
            self.sim_accum -= step;
            steps += 1;
        }
        let sim_overrun = steps == self.cfg.max_sim_steps && self.sim_accum >= step;
        if sim_overrun {
            // Se descarta el tiempo restante: mejor un frame "lento" que una
            // espiral de muerte. El scheduler lo registra como `sim_overrun`.
            self.sim_accum = Duration::ZERO;
        }
        let alpha = (self.sim_accum.as_secs_f32() / step.as_secs_f32()).clamp(0.0, 1.0);
        self.world.interpolate(alpha);

        // ── 3. Frame XR/estéreo ──────────────────────────────────────────────
        let (width, height) = (self.gpu.config.width, self.gpu.config.height);
        let aspect = width as f32 / height.max(1) as f32;
        let stereo = self
            .xr
            .begin_frame(self.world.head_pose(), aspect, now)
            .context("begin_frame XR falló")?;

        // ── 4. Streaming de geometría (C++), una vez por frame ───────────────
        let view = View::from_frame(&stereo, width, height);
        self.geometry
            .begin_frame(&view, self.frame_id)
            .context("begin_frame de geometría falló")?;

        let mut requests = [PageRequest::ZERO; 128];
        let _requested = self.geometry.collect(&mut requests);

        // Las páginas listas se suben a GPU en Rust (wgpu manda); C++ nunca
        // toca objetos de wgpu. `drain_ready` devuelve punteros válidos hasta
        // `ack_upload`.
        let mut ready = [ReadyPage::ZERO; 64];
        let count = self.geometry.drain_ready(&mut ready);
        for page in &ready[..count] {
            // SAFETY: el puntero es válido hasta `ack_upload`, que se llama
            // inmediatamente después de consumir el slice.
            let bytes = unsafe { page.as_slice() };
            self.renderer.upload_page(&self.gpu.queue, bytes);
            self.geometry
                .ack_upload(page.page_id)
                .context("ack_upload de geometría falló")?;
        }

        // ── 5. Render ────────────────────────────────────────────────────────
        self.renderer.update(
            &self.gpu.queue,
            width,
            height,
            self.elapsed.as_secs_f32(),
            &stereo,
        );
        let surface_recovered = self.render(stereo.stereo)?;

        // ── 6. Telemetría y pacing ───────────────────────────────────────────
        let frame_us = frame_started.elapsed().as_micros().min(u32::MAX as u128) as u32;
        self.metrics.record(FrameSample {
            frame_us,
            sim_steps: steps.min(u8::MAX as u32) as u8,
            sim_overrun,
            surface_recovered,
        });
        let on_time = self.xr.end_frame(frame_us);
        if !on_time {
            // La sesión XR ya aplica control adaptativo; aquí solo se deja
            // constancia en la telemetría agregada.
        }

        if let Some(pacer) = &mut self.pacer {
            pacer.on_frame(now);
        }

        if self.cfg.telemetry_every > 0
            && self.frame_id % self.cfg.telemetry_every == 0
            && self.frame_id > 0
        {
            self.emit_telemetry();
        }

        self.frame_id = self.frame_id.saturating_add(1);
        Ok(())
    }

    fn render(&mut self, stereo: bool) -> Result<bool> {
        let frame = match self.gpu.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.gpu.recover();
                return Ok(true);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(wgpu::SurfaceError::OutOfMemory) => {
                anyhow::bail!("GPU sin memoria: no se puede continuar")
            }
            Err(err) => anyhow::bail!("error de superficie: {err:?}"),
        };

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tlaxtrelom.frame"),
            });
        self.renderer.draw(
            &mut encoder,
            &view,
            &self.gpu.depth_view,
            self.gpu.config.width,
            self.gpu.config.height,
            stereo,
        );
        self.gpu.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(false)
    }

    fn emit_telemetry(&self) {
        let metrics = self.metrics.snapshot();
        let bci = self.fulldive.health();
        let xr = self.xr.stats();
        let geo = self.geometry.stats();
        eprintln!(
            "[tlaxtrelom] {metrics} | xr late={} scale={:.2} | \
             bci frames={} gaps={} artifacts={} last={}us max={}us rt={} pinned={} locked={} | \
             geo pages={} io={}B queue={}",
            xr.late_frames,
            self.xr.render_scale(),
            bci.frames,
            bci.gaps,
            bci.artifacts,
            bci.last_us,
            bci.max_us,
            bci.rt_scheduler,
            bci.pinned,
            bci.locked_memory,
            geo.pages_resident,
            geo.io_bytes,
            geo.io_queue_depth,
        );
    }

    fn resize(&mut self, width: u32, height: u32) {
        if self.gpu.resize(width, height) {
            self.xr.reset();
            if let Some(pacer) = &mut self.pacer {
                pacer.reset(Instant::now());
            }
        }
    }

    fn reset(&mut self) {
        self.world.reset();
        self.gate.reset();
        self.xr.reset();
        self.sim_accum = Duration::ZERO;
        self.last_frame = Instant::now();
    }

    fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        self.gate.reset();
        let now = Instant::now();
        self.last_frame = now;
        if let Some(pacer) = &mut self.pacer {
            pacer.reset(now);
        }
    }

    fn on_focus_lost(&mut self) {
        // Nunca dejar una intención ni teclas pegadas al perder foco
        self.input = InputState::default();
        self.gate.reset();
    }

    fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        if self.fullscreen {
            self.window
                .set_fullscreen(Some(Fullscreen::Borderless(None)));
        } else {
            self.window.set_fullscreen(None);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Integración con winit 0.30
// ─────────────────────────────────────────────────────────────────────────────

struct App {
    cfg: Config,
    engine: Option<Engine>,
}

impl App {
    fn new(cfg: Config) -> Self {
        Self { cfg, engine: None }
    }

    fn window_title(&self) -> &'static str {
        match self.cfg.mode {
            RunMode::Windowed => "Tlaxtrelom Engine — runtime VR/FullDive",
            RunMode::StereoPreview => "Tlaxtrelom Engine — preview estéreo",
            RunMode::Headless => "Tlaxtrelom Engine — headless",
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title(self.window_title())
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(320.0, 240.0));

        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("[tlaxtrelom] no se pudo crear la ventana: {err}");
                event_loop.exit();
                return;
            }
        };

        match Engine::new(window, self.cfg.clone()) {
            Ok(engine) => {
                eprintln!(
                    "[tlaxtrelom] inicio modo={:?} xr={:?} fps={} sim={}Hz",
                    self.cfg.mode,
                    engine.xr.config().mode,
                    self.cfg.target_fps,
                    self.cfg.sim_hz
                );
                self.engine = Some(engine);
            }
            Err(err) => {
                eprintln!("[tlaxtrelom] init falló: {err:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(engine) = &mut self.engine {
                    engine.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                if let Some(engine) = &mut self.engine {
                    engine.on_focus_lost();
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(engine) = &mut self.engine {
                    engine.set_paused(occluded);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if pressed {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => event_loop.exit(),
                        Key::Character(text) => match text.as_str() {
                            "p" | "P" => {
                                if let Some(engine) = &mut self.engine {
                                    let paused = !engine.paused;
                                    engine.set_paused(paused);
                                }
                            }
                            "r" | "R" => {
                                if let Some(engine) = &mut self.engine {
                                    engine.reset();
                                }
                            }
                            "f" | "F" => {
                                if let Some(engine) = &mut self.engine {
                                    engine.toggle_fullscreen();
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }

                // Control interactivo de navegación WASD / Flechas / Espacio / Shift
                if let Some(engine) = &mut self.engine {
                    match &event.logical_key {
                        Key::Character(text) => match text.to_lowercase().as_str() {
                            "w" => engine.input.forward = pressed,
                            "s" => engine.input.backward = pressed,
                            "a" => engine.input.left = pressed,
                            "d" => engine.input.right = pressed,
                            "q" => engine.input.turn_left = pressed,
                            "e" => engine.input.turn_right = pressed,
                            _ => {}
                        },
                        Key::Named(NamedKey::ArrowUp) => engine.input.forward = pressed,
                        Key::Named(NamedKey::ArrowDown) => engine.input.backward = pressed,
                        Key::Named(NamedKey::ArrowLeft) => engine.input.turn_left = pressed,
                        Key::Named(NamedKey::ArrowRight) => engine.input.turn_right = pressed,
                        Key::Named(NamedKey::Shift) => engine.input.sprint = pressed,
                        Key::Named(NamedKey::Space) => engine.input.action = pressed,
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(engine) = &mut self.engine {
                    if button == MouseButton::Left {
                        engine.input.mouse_down = state == ElementState::Pressed;
                        if !engine.input.mouse_down {
                            engine.input.last_cursor = None;
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(engine) = &mut self.engine {
                    if engine.input.mouse_down {
                        if let Some((last_x, _)) = engine.input.last_cursor {
                            let dx = (position.x - last_x) as f32;
                            // Giro horizontal relativo al arrastrar con el ratón
                            engine.input.mouse_yaw_delta += dx * 0.003;
                        }
                        engine.input.last_cursor = Some((position.x, position.y));
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(engine) = &mut self.engine else {
                    return;
                };
                match engine.tick() {
                    Ok(()) => engine.consecutive_errors = 0,
                    Err(err) => {
                        engine.metrics.record_error();
                        engine.consecutive_errors = engine.consecutive_errors.saturating_add(1);
                        eprintln!(
                            "[tlaxtrelom] tick falló ({}/{}): {err:#}",
                            engine.consecutive_errors, engine.cfg.max_consecutive_errors
                        );
                        if engine.consecutive_errors >= engine.cfg.max_consecutive_errors {
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = &self.engine else {
            event_loop.exit();
            return;
        };
        engine.window.request_redraw();
        match engine.next_deadline() {
            Some(deadline) if !engine.paused => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            _ => event_loop.set_control_flow(ControlFlow::Poll),
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!(
            "tlaxtrelom {VERSION} — Tlaxtrelom Engine ({})",
            std::env::consts::OS
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--probe") {
        for line in renderer::probe()? {
            println!("{line}");
        }
        return Ok(());
    }

    let mut cfg = Config::parse(args.into_iter().skip(1))?;
    cfg.apply_xr_to_fps();
    cfg.validate()?;

    if cfg.mode.is_headless() {
        let report = selftest(&cfg)?;
        if cfg.json {
            print!("{}", report.to_json());
        } else {
            println!("{report}");
        }
        if !report.is_ok() {
            std::process::exit(2);
        }
        return Ok(());
    }

    let event_loop =
        EventLoop::new().map_err(|e| anyhow::anyhow!("no se pudo crear el event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(cfg);
    event_loop
        .run_app(&mut app)
        .map_err(|e| anyhow::anyhow!("event loop terminó con error: {e}"))?;
    Ok(())
}
