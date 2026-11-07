// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! # Pila FullDive (BCI)
//!
//! ```text
//! NeuralSource ──► NeuralBatch ──► Decoder ──► IntentFrame ──► triple buffer
//!   (USB/replay)      (preasignado)  (DSP+ML)    (POD, 1 kHz)     (wait-free)
//! ```
//!
//! Contrato del hilo de tiempo real:
//! - cero asignaciones en `poll`/`push`/`write` (todo preasignado);
//! - cero syscalls y cero logging en el camino caliente;
//! - cero locks compartidos con el render (triple buffer + SPSC ring);
//! - `SCHED_FIFO` + `mlockall` + CPU pinning cuando el sistema lo permite.
//!
//! El decoder de referencia usa biquads + potencia + un modelo lineal. Es
//! deliberadamente pequeño y determinista: el contrato (`IntentFrame`) es lo
//! que importa, no la sofisticación del modelo. Un decoder CSP/Riemann/TCN
//! real se enchufa sin tocar el resto del motor.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rtrb::{Consumer, Producer, RingBuffer};
use triple_buffer::{triple_buffer, Input, Output};

/// Watchdog del lado del render: sin frame nuevo en 50 ms ⇒ neutral.
pub const WATCHDOG: Duration = Duration::from_millis(50);

/// Canales máximos del decoder de referencia (un array fijo evita allocs).
pub const MAX_CHANNELS: usize = 64;

/// Umbral de artefacto (µV equivalentes). Un electrodo caído satura el ADC.
const ARTIFACT_UV: f32 = 200.0;

// ─────────────────────────────────────────────────────────────────────────────
// Tipos públicos
// ─────────────────────────────────────────────────────────────────────────────

/// Resultado del decoder. `#[repr(C)]`, 56 bytes: una línea de caché.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntentFrame {
    /// Timestamp del último sample usado (ns, reloj del host).
    pub t_ns: u64,
    /// Secuencia del decoder; permite detectar frames repetidos.
    pub seq: u32,
    /// Bitmask de `flags`.
    pub flags: u32,
    /// Confianza calibrada [0, 1].
    pub confidence: f32,
    /// 0..3: ejes continuos. 4..7: velocidad de esos ejes (predicción).
    pub axes: [f32; 8],
    /// Acciones discretas (clic, agarre, menú). Las valida el `IntentGate`.
    pub actions: u16,
    pub _pad: u16,
}

impl IntentFrame {
    pub const fn neutral() -> Self {
        Self {
            t_ns: 0,
            seq: 0,
            flags: 0,
            confidence: 0.0,
            axes: [0.0; 8],
            actions: 0,
            _pad: 0,
        }
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.flags & flags::VALID != 0
    }

    /// Invalida el frame de forma segura (útil para el watchdog y tests).
    pub fn neutralized(mut self) -> Self {
        self.axes = [0.0; 8];
        self.actions = 0;
        self.flags &= !flags::VALID;
        self
    }
}

pub mod flags {
    // API pública del `IntentFrame`; el editor y el gameplay leen estos bits.
    #![allow(dead_code)]

    pub const VALID: u32 = 1 << 0;
    pub const CALIBRATING: u32 = 1 << 1;
    pub const ARTIFACT: u32 = 1 << 2;
    pub const GATED: u32 = 1 << 3;
    pub const PREDICTED: u32 = 1 << 4;
    pub const ACTION: u32 = 1 << 5;
}

/// Evento discreto (clic, menú, "soltar"). Va por un ring SPSC para no perder
/// pulsos si el render se salta un frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NeuroEvent {
    pub kind: u16,
    pub code: u16,
    pub t_ns: u64,
    pub value: f32,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub channels: u16,
    pub sample_rate: u32,
    pub core_id: usize,
    pub mock: bool,
    /// Si es `false`, no se abre transporte ni se crea hilo BCI. El motor ve
    /// intención neutral: es el modo seguro para menús, editores y CI.
    pub enabled: bool,
    pub device: String,
    /// Si existe, se reproduce un log `.tlxneuro` en lugar de abrir hardware.
    pub replay: Option<PathBuf>,
    /// Prioridad `SCHED_FIFO`. 0 = no intentar RT.
    pub rt_priority: i32,
    /// Ventana de calibración inicial (el decoder no emite `VALID` antes).
    pub calibration_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channels: 8,
            sample_rate: 1_000,
            core_id: 3,
            mock: true,
            enabled: true,
            device: "/dev/tlaxtrelom-neuro0".into(),
            replay: None,
            rt_priority: 80,
            calibration_ms: 2_000,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.channels == 0 || self.channels as usize > MAX_CHANNELS {
            bail!("channels fuera de [1, {}]: {}", MAX_CHANNELS, self.channels);
        }
        if !(250..=16_000).contains(&self.sample_rate) {
            bail!("sample_rate fuera de [250, 16000]: {}", self.sample_rate);
        }
        if !(0..=99).contains(&self.rt_priority) {
            bail!("rt_priority fuera de [0, 99]: {}", self.rt_priority);
        }
        if self.calibration_ms > 10_000 {
            bail!("calibration_ms demasiado alto: {}", self.calibration_ms);
        }
        if let Some(path) = &self.replay {
            if !path.exists() {
                bail!("no existe el archivo de replay: {}", path.display());
            }
        }
        Ok(())
    }
}

/// Lote de muestras intercaladas. `samples` se asigna una vez; `poll` solo
/// escribe dentro y actualiza `len`. Nunca reasigna en el camino caliente.
pub struct NeuralBatch {
    pub channels: usize,
    pub sample_rate: u32,
    pub t0_ns: u64,
    pub seq: u32,
    pub samples: Vec<f32>,
    pub len: usize,
}

impl NeuralBatch {
    pub fn with_capacity(channels: usize, max_samples: usize) -> Self {
        Self {
            channels,
            sample_rate: 0,
            t0_ns: 0,
            seq: 0,
            samples: vec![0.0; channels * max_samples],
            len: 0,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn max_samples(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
}

/// Fuente de muestras. `poll` puede bloquear (lectura USB con timeout) o
/// devolver 0 si no hay datos. Nunca asigna en el camino caliente.
pub trait NeuralSource: Send {
    fn poll(&mut self, out: &mut NeuralBatch) -> Result<usize>;

    /// Secuencia del transporte; `None` si la fuente no la expone.
    fn sequence(&self) -> Option<u32> {
        None
    }

    /// Frecuencia real de la fuente (un replay puede diferir de la config).
    fn sample_rate_hint(&self) -> Option<u32> {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handle del hilo + lectura wait-free
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Stats {
    pub frames: AtomicU64,
    pub errors: AtomicU64,
    pub gaps: AtomicU64,
    pub artifacts: AtomicU64,
    pub events_dropped: AtomicU64,
    pub last_us: AtomicU32,
    pub max_us: AtomicU32,
    pub rt_scheduler: AtomicBool,
    pub pinned: AtomicBool,
    pub locked_memory: AtomicBool,
}

impl Stats {
    fn record(&self, us: u32) {
        self.last_us.store(us, Ordering::Relaxed);
        let mut max = self.max_us.load(Ordering::Relaxed);
        while us > max {
            match self
                .max_us
                .compare_exchange_weak(max, us, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(observed) => max = observed,
            }
        }
    }
}

/// Vista inmutable y agregada del estado del subsistema BCI. La usa el HUD, la
/// telemetría y el watchdog del host; no toca el hilo RT.
#[derive(Debug, Clone, Copy, Default)]
pub struct Health {
    pub running: bool,
    pub frames: u64,
    pub errors: u64,
    pub gaps: u64,
    pub artifacts: u64,
    pub events_dropped: u64,
    pub last_us: u32,
    pub max_us: u32,
    pub rt_scheduler: bool,
    pub pinned: bool,
    pub locked_memory: bool,
}

pub struct FullDiveHandle {
    intent: Output<IntentFrame>,
    events: Consumer<NeuroEvent>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    pub stats: Arc<Stats>,
    last_rx: Instant,
    last_seq: u32,
}

impl FullDiveHandle {
    pub fn spawn(cfg: Config) -> Result<Self> {
        cfg.validate()?;

        // `triple_buffer` es wait-free para el productor y latest-wins para el
        // consumidor: el render nunca espera al hilo BCI.
        let (intent_w, intent_r) = triple_buffer(&IntentFrame::neutral());
        let (ev_tx, ev_rx) = RingBuffer::new(256);
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::default());

        if !cfg.enabled {
            // Sin transporte: la intención se queda neutral para siempre.
            drop(intent_w);
            drop(ev_tx);
            return Ok(Self {
                intent: intent_r,
                events: ev_rx,
                stop,
                join: None,
                stats,
                last_rx: Instant::now(),
                last_seq: 0,
            });
        }

        let (stop2, stats2) = (stop.clone(), stats.clone());
        let join = thread::Builder::new()
            .name("tlaxtrelom-fulldive".into())
            .stack_size(1 << 20)
            .spawn(move || {
                if let Err(e) = run(cfg, intent_w, ev_tx, stop2, stats2) {
                    // Log solo en el arranque/error, nunca por muestra.
                    eprintln!("[fulldive] hilo terminó: {e:#}");
                }
            })
            .context("no se pudo crear el hilo FullDive")?;

        Ok(Self {
            intent: intent_r,
            events: ev_rx,
            stop,
            join: Some(join),
            stats,
            last_rx: Instant::now(),
            last_seq: 0,
        })
    }

    /// Último intent disponible. No bloquea. Si no hay frame nuevo en
    /// `WATCHDOG`, devuelve neutral para que el juego no use datos viejos.
    pub fn snapshot(&mut self, now: Instant) -> IntentFrame {
        let frame = *self.intent.read();
        if frame.seq != self.last_seq {
            self.last_seq = frame.seq;
            self.last_rx = now;
        }
        if now.saturating_duration_since(self.last_rx) > WATCHDOG {
            return frame.neutralized();
        }
        frame
    }

    pub fn next_event(&mut self) -> Option<NeuroEvent> {
        self.events.pop().ok()
    }

    pub fn health(&self) -> Health {
        Health {
            running: self
                .join
                .as_ref()
                .map(|j| !j.is_finished())
                .unwrap_or(false),
            frames: self.stats.frames.load(Ordering::Relaxed),
            errors: self.stats.errors.load(Ordering::Relaxed),
            gaps: self.stats.gaps.load(Ordering::Relaxed),
            artifacts: self.stats.artifacts.load(Ordering::Relaxed),
            events_dropped: self.stats.events_dropped.load(Ordering::Relaxed),
            last_us: self.stats.last_us.load(Ordering::Relaxed),
            max_us: self.stats.max_us.load(Ordering::Relaxed),
            rt_scheduler: self.stats.rt_scheduler.load(Ordering::Relaxed),
            pinned: self.stats.pinned.load(Ordering::Relaxed),
            locked_memory: self.stats.locked_memory.load(Ordering::Relaxed),
        }
    }
}

impl Drop for FullDiveHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            // El contrato de `NeuralSource` exige timeout de lectura; si no se
            // cumple, este join puede colgar. Es Deliberado: preferimos un
            // cierre limpio a un hilo huérfano tocando hardware.
            let _ = join.join();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Hilo de tiempo real
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
struct RtStatus {
    pinned: bool,
    scheduler: bool,
    locked_memory: bool,
}

fn run(
    cfg: Config,
    mut out: Input<IntentFrame>,
    mut events: Producer<NeuroEvent>,
    stop: Arc<AtomicBool>,
    stats: Arc<Stats>,
) -> Result<()> {
    let rt = pin_realtime(cfg.core_id, cfg.rt_priority);
    stats.pinned.store(rt.pinned, Ordering::Relaxed);
    stats.rt_scheduler.store(rt.scheduler, Ordering::Relaxed);
    stats
        .locked_memory
        .store(rt.locked_memory, Ordering::Relaxed);

    let mut source = open_source(&cfg)?;
    let sample_rate = source
        .sample_rate_hint()
        .unwrap_or(cfg.sample_rate)
        .clamp(250, 16_000);
    let mut decoder =
        Decoder::with_calibration(cfg.channels as usize, sample_rate, cfg.calibration_ms);
    let mut batch = NeuralBatch::with_capacity(cfg.channels as usize, 256);
    let mut last_src_seq: Option<u32> = None;
    let mut error_logged = false;

    while !stop.load(Ordering::Relaxed) {
        if let Some(seq) = source.sequence() {
            if let Some(prev) = last_src_seq {
                if seq > prev.saturating_add(1) {
                    stats
                        .gaps
                        .fetch_add((seq - prev - 1) as u64, Ordering::Relaxed);
                }
            }
            last_src_seq = Some(seq);
        }

        batch.clear();
        match source.poll(&mut batch) {
            Ok(0) => {
                // Sin datos: espera activa corta. En un transporte real, `poll`
                // bloquea en `read` con timeout y esto casi nunca ocurre.
                std::hint::spin_loop();
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                stats.errors.fetch_add(1, Ordering::Relaxed);
                if !error_logged {
                    eprintln!("[fulldive] error de transporte: {e:#}");
                    error_logged = true;
                }
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        }

        let t0 = Instant::now();
        let frame = decoder.push(&batch);
        // `as_micros()` puede exceder u32 en un stall patológico; saturar es
        // mejor que truncar y reportar una latencia falsamente baja.
        let elapsed_us = t0.elapsed().as_micros().min(u32::MAX as u128) as u32;
        stats.record(elapsed_us);

        if frame.flags & flags::ARTIFACT != 0 {
            stats.artifacts.fetch_add(1, Ordering::Relaxed);
        }

        // Publicación wait-free. Si el render no consume, se sobrescribe el
        // valor anterior: para input continuo, "último gana" es lo correcto.
        out.write(frame);

        for ev in decoder.drain_events() {
            if events.push(*ev).is_err() {
                stats.events_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        decoder.clear_events();
        stats.frames.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn open_source(cfg: &Config) -> Result<Box<dyn NeuralSource>> {
    if let Some(path) = &cfg.replay {
        return Ok(Box::new(ReplaySource::open(path)?));
    }
    if cfg.mock {
        return Ok(Box::new(MockSource::new(cfg.channels, cfg.sample_rate)));
    }
    // Sin mock y sin replay: solo hardware real. Si no está, el hilo termina
    // con error y el motor sigue con intención neutral. Nunca se inventan
    // señales cuando el modo seguro está activo.
    match UsbSource::open(&cfg.device) {
        Ok(s) => Ok(Box::new(s)),
        Err(e) => Err(e.context("transporte BCI no disponible en modo seguro")),
    }
}

fn pin_realtime(core: usize, priority: i32) -> RtStatus {
    let mut status = RtStatus::default();
    #[cfg(target_os = "linux")]
    // SAFETY: las llamadas siguientes operan sobre el hilo actual y sobre
    // `set`, inicializado con `zeroed` y poblado por `CPU_ZERO`/`CPU_SET`.
    // `sched_setscheduler` y `mlockall` no retienen punteros al stack.
    unsafe {
        // CPU pinning: evita migraciones y jitter de caché.
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if core < cores {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut set);
            libc::CPU_SET(core, &mut set);
            status.pinned =
                libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0;
        }

        // SCHED_FIFO requiere CAP_SYS_NICE; si falla, el hilo sigue en CFS y la
        // telemetría lo refleja (`rt_scheduler = false`). No es fatal.
        if priority > 0 {
            let param = libc::sched_param {
                sched_priority: priority,
            };
            status.scheduler = libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) == 0;
        }

        // Bloquea la memoria del hilo: evita page faults en el camino caliente.
        status.locked_memory = libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) == 0;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (core, priority);
    }
    status
}

// ─────────────────────────────────────────────────────────────────────────────
// Transportes
// ─────────────────────────────────────────────────────────────────────────────

struct UsbSource {
    _file: File,
}

impl UsbSource {
    fn open(path: &str) -> Result<Self> {
        // Producción: `nusb` + ring mmap + parser de `tlaxtrelom-neuro-proto`.
        // Se deja explícito para que la demo no finja hardware.
        bail!("transporte USB no compilado en esta demo ({path})")
    }
}

impl NeuralSource for UsbSource {
    fn poll(&mut self, _out: &mut NeuralBatch) -> Result<usize> {
        bail!("UsbSource no disponible")
    }
}

/// Generador sintético determinista: alpha 10 Hz + beta 20 Hz + ruido LCG.
/// Permite correr el motor y los tests de latencia sin electrodos.
struct MockSource {
    channels: usize,
    sample_rate: u32,
    phase: f32,
    rng: u64,
    t0: Instant,
    seq: u32,
}

impl MockSource {
    fn new(channels: u16, sample_rate: u32) -> Self {
        Self {
            channels: channels as usize,
            sample_rate,
            phase: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
            t0: Instant::now(),
            seq: 0,
        }
    }
}

impl NeuralSource for MockSource {
    fn poll(&mut self, out: &mut NeuralBatch) -> Result<usize> {
        let n = 1; // 1 muestra/canal por llamada a 1 kHz
        let ch = self.channels.min(out.channels);
        let dt = 1.0 / self.sample_rate as f32;
        for s in 0..n {
            for c in 0..ch {
                let alpha = (self.phase * std::f32::consts::TAU * 10.0).sin();
                let beta = (self.phase * std::f32::consts::TAU * 20.0).sin();
                self.rng = self
                    .rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let noise = ((self.rng >> 33) as f32 / u32::MAX as f32 - 0.5) * 2.0;
                out.samples[s * out.channels + c] = 8.0 * alpha + 3.0 * beta + noise;
            }
            self.phase += dt;
        }
        out.len = n;
        out.sample_rate = self.sample_rate;
        out.t0_ns = self.t0.elapsed().as_nanos() as u64;
        out.seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        Ok(n)
    }

    fn sequence(&self) -> Option<u32> {
        Some(self.seq)
    }

    fn sample_rate_hint(&self) -> Option<u32> {
        Some(self.sample_rate)
    }
}

/// Reproducción determinista de un log `.tlxneuro`:
///
/// ```text
/// magic:        [u8; 8] = "TLXNEUR1"
/// channels:     u16 LE
/// sample_rate:  u32 LE
/// frames:       u32 LE
/// payload:      frames * channels * f32 LE intercalado
/// ```
struct ReplaySource {
    file: File,
    channels: u16,
    sample_rate: u32,
    remaining_frames: u32,
    seq: u32,
    t0_ns: u64,
    buf: Vec<u8>,
}

impl ReplaySource {
    fn open(path: &Path) -> Result<Self> {
        let mut file =
            File::open(path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
        let mut header = [0u8; 18];
        file.read_exact(&mut header)
            .context("cabecera .tlxneuro truncada")?;
        if &header[..8] != b"TLXNEUR1" {
            bail!("magic inválido en {}: se esperaba TLXNEUR1", path.display());
        }
        let channels = u16::from_le_bytes([header[8], header[9]]);
        let sample_rate = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);
        let frames = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
        if channels == 0 || channels as usize > MAX_CHANNELS {
            bail!("canales inválidos en replay: {channels}");
        }
        if !(250..=16_000).contains(&sample_rate) {
            bail!("sample_rate inválido en replay: {sample_rate}");
        }
        Ok(Self {
            file,
            channels,
            sample_rate,
            remaining_frames: frames,
            seq: 0,
            t0_ns: 0,
            // 256 muestras * 64 canales * 4 B = 64 KiB: el máximo que `poll`
            // puede pedir en una llamada. Se asigna una vez, fuera del camino
            // caliente.
            buf: Vec::with_capacity(64 * 1024),
        })
    }
}

impl NeuralSource for ReplaySource {
    fn poll(&mut self, out: &mut NeuralBatch) -> Result<usize> {
        if self.remaining_frames == 0 {
            return Ok(0);
        }
        let src_ch = self.channels as usize;
        let dst_ch = out.channels.max(1);
        let n = self.remaining_frames.min(out.max_samples() as u32).min(256) as usize;
        if n == 0 {
            return Ok(0);
        }

        let bytes = n * src_ch * 4;
        self.buf.resize(bytes, 0);
        match self.file.read_exact(&mut self.buf[..bytes]) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.remaining_frames = 0;
                return Ok(0);
            }
            Err(e) => return Err(e.into()),
        }

        for frame in 0..n {
            for ch in 0..src_ch.min(dst_ch) {
                let off = (frame * src_ch + ch) * 4;
                let raw = [
                    self.buf[off],
                    self.buf[off + 1],
                    self.buf[off + 2],
                    self.buf[off + 3],
                ];
                out.samples[frame * dst_ch + ch] = f32::from_le_bytes(raw);
            }
        }

        self.remaining_frames -= n as u32;
        out.len = n;
        out.sample_rate = self.sample_rate;
        out.t0_ns = self.t0_ns;
        out.seq = self.seq;
        self.t0_ns = self
            .t0_ns
            .saturating_add((n as u64 * 1_000_000_000) / self.sample_rate as u64);
        self.seq = self.seq.wrapping_add(1);
        Ok(n)
    }

    fn sequence(&self) -> Option<u32> {
        Some(self.seq)
    }

    fn sample_rate_hint(&self) -> Option<u32> {
        Some(self.sample_rate)
    }
}

/// Escribe un log de replay a partir de muestras intercaladas. La usa el cooker
/// y los tests de determinismo; nunca el frame loop.
pub fn write_replay_log(
    path: &Path,
    channels: u16,
    sample_rate: u32,
    samples: &[f32],
) -> Result<()> {
    if channels == 0 || channels as usize > MAX_CHANNELS {
        bail!("canales inválidos: {channels}");
    }
    let ch = channels as usize;
    if !(250..=16_000).contains(&sample_rate) {
        bail!("sample_rate inválido: {sample_rate}");
    }
    if samples.len() % ch != 0 {
        bail!("las muestras no son múltiplo de {ch} canales");
    }
    let frames = samples.len() / ch;
    if frames > u32::MAX as usize {
        bail!("demasiados frames para el formato: {frames}");
    }
    let frames = frames as u32;

    let mut f =
        File::create(path).with_context(|| format!("no se pudo crear {}", path.display()))?;
    f.write_all(b"TLXNEUR1")?;
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&frames.to_le_bytes())?;
    for sample in samples {
        f.write_all(&sample.to_le_bytes())?;
    }
    f.sync_all()?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Decoder de referencia (DSP + modelo lineal)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    const fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Notch RBJ. `q` alto = banda estrecha (30 ≈ 1.7 Hz a 50 Hz).
    fn notch(sr: f32, f0: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * f0 / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: 1.0 / a0,
            b1: -2.0 * cos / a0,
            b2: 1.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Bandpass RBJ de "skirt constante".
    fn bandpass(sr: f32, f0: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * f0 / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        if y.is_finite() {
            y
        } else {
            // Un filtro inestable por datos corruptos no debe envenenar el
            // estado del decoder: se reinicia y se devuelve 0.
            self.z1 = 0.0;
            self.z2 = 0.0;
            0.0
        }
    }
}

pub struct Decoder {
    channels: usize,
    sample_rate: u32,
    notch: [Biquad; MAX_CHANNELS],
    band: [Biquad; MAX_CHANNELS],
    power: [f32; MAX_CHANNELS],
    ema: f32,
    weights: [[f32; MAX_CHANNELS]; 4],
    bias: [f32; 4],
    prev_axes: [f32; 4],
    seq: u32,
    seen: u64,
    calibration_samples: u64,
    events: [NeuroEvent; 8],
    event_len: usize,
}

impl Decoder {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        Self::with_calibration(channels, sample_rate, 0)
    }

    pub fn with_calibration(channels: usize, sample_rate: u32, calibration_ms: u32) -> Self {
        let ch = channels.clamp(1, MAX_CHANNELS);
        let sr = sample_rate.clamp(250, 16_000) as f32;
        let mut notch = [Biquad::identity(); MAX_CHANNELS];
        let mut band = [Biquad::identity(); MAX_CHANNELS];
        for c in 0..ch {
            // Notch de línea (50 Hz; en América, 60 Hz) y banda mu/beta.
            notch[c] = Biquad::notch(sr, 50.0, 30.0);
            band[c] = Biquad::bandpass(sr, 12.0, 1.2);
        }

        // Pesos de demo: canales pares → eje X, impares → eje Y. En producción
        // salen de CSP/Riemann + LDA entrenado por usuario.
        let mut weights = [[0.0f32; MAX_CHANNELS]; 4];
        for (c, weight) in weights[0].iter_mut().enumerate().take(ch) {
            *weight = if c % 2 == 0 { 1.0 } else { 0.0 };
        }
        for (c, weight) in weights[1].iter_mut().enumerate().take(ch) {
            *weight = if c % 2 == 1 { 1.0 } else { 0.0 };
        }

        let calibration_samples = (sr * calibration_ms as f32 / 1000.0) as u64;
        Self {
            channels: ch,
            sample_rate: sample_rate.clamp(250, 16_000),
            notch,
            band,
            power: [0.0; MAX_CHANNELS],
            // EMA de potencia con tau = 250 ms.
            ema: 1.0 - (-1.0 / (sr * 0.25)).exp(),
            weights,
            bias: [0.0; 4],
            prev_axes: [0.0; 4],
            seq: 0,
            seen: 0,
            calibration_samples,
            events: [NeuroEvent {
                kind: 0,
                code: 0,
                t_ns: 0,
                value: 0.0,
            }; 8],
            event_len: 0,
        }
    }

    /// Procesa un lote y devuelve el intent. No asigna memoria.
    pub fn push(&mut self, b: &NeuralBatch) -> IntentFrame {
        self.event_len = 0;
        let mut frame = IntentFrame::neutral();
        frame.seq = self.seq.wrapping_add(1);
        self.seq = frame.seq;

        let ch = b.channels.min(self.channels);
        let n = b.len.min(b.max_samples());
        let rate_ok = b.sample_rate == 0 || b.sample_rate == self.sample_rate;
        if ch == 0 || n == 0 || !rate_ok {
            frame.flags = flags::ARTIFACT;
            if !rate_ok {
                self.push_artifact_event(b.t0_ns, 2);
            }
            return frame;
        }

        let mut artifact = false;
        for s in 0..n {
            for c in 0..ch {
                let raw = b.samples[s * b.channels + c];
                let x = if raw.is_finite() {
                    raw
                } else {
                    artifact = true;
                    0.0
                };
                if x.abs() > ARTIFACT_UV {
                    artifact = true;
                }
                let x = x.clamp(-ARTIFACT_UV, ARTIFACT_UV);
                // Dos biquads en cascada. Se separan las líneas para que el
                // borrow checker vea préstamos disjuntos de campos.
                let filtered = self.notch[c].process(x);
                let y = self.band[c].process(filtered);
                self.power[c] += self.ema * (y * y - self.power[c]);
            }
        }
        self.seen = self.seen.saturating_add(n as u64);
        let calibrating = self.seen < self.calibration_samples;

        let mut axes = [0.0f32; 8];
        let mut confidence = 0.0f32;
        if !calibrating {
            for (k, axis) in axes.iter_mut().enumerate().take(4) {
                let mut acc = self.bias[k];
                for (weight, power) in self.weights[k][..ch].iter().zip(&self.power[..ch]) {
                    acc += weight * power.max(0.0).sqrt();
                }
                *axis = acc.tanh();
            }

            // Confianza grosera: SNR proxy + penalización por artefacto.
            let mean_power = self.power[..ch].iter().sum::<f32>() / ch as f32;
            confidence = (mean_power.sqrt() / 20.0).clamp(0.0, 1.0);
            if artifact {
                confidence = 0.0;
                self.push_artifact_event(b.t0_ns, 1);
            }

            // Velocidad estimada por diferencia finita (para predicción).
            for k in 0..4 {
                let vel = (axes[k] - self.prev_axes[k]) * 10.0;
                axes[k + 4] = vel.clamp(-1.0, 1.0);
                self.prev_axes[k] = axes[k];
            }
        }

        let mut flag_bits = 0;
        if calibrating {
            flag_bits |= flags::CALIBRATING;
        }
        if artifact {
            flag_bits |= flags::ARTIFACT;
        }
        if confidence >= 0.35 && !artifact && !calibrating {
            flag_bits |= flags::VALID;
        }

        let t_ns = b.t0_ns + (n as u64 * 1_000_000_000) / self.sample_rate.max(1) as u64;

        IntentFrame {
            t_ns,
            seq: self.seq,
            flags: flag_bits,
            confidence,
            axes,
            actions: 0,
            _pad: 0,
        }
    }

    fn push_artifact_event(&mut self, t_ns: u64, code: u16) {
        if self.event_len < self.events.len() {
            self.events[self.event_len] = NeuroEvent {
                kind: 1,
                code,
                t_ns,
                value: 0.0,
            };
            self.event_len += 1;
        }
    }

    #[inline]
    pub fn drain_events(&self) -> &[NeuroEvent] {
        &self.events[..self.event_len]
    }

    #[inline]
    pub fn clear_events(&mut self) {
        self.event_len = 0;
    }

    pub fn calibration_fraction(&self) -> f32 {
        if self.calibration_samples == 0 {
            1.0
        } else {
            (self.seen as f32 / self.calibration_samples as f32).clamp(0.0, 1.0)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FullDive Profesional: Telemetría Biológica, Seguridad Somatosensorial y E-Stop
// ─────────────────────────────────────────────────────────────────────────────

/// Vectores biológicos del usuario para monitoreo de seguridad en tiempo real.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiometricVitals {
    pub heart_rate_bpm: f32,
    pub blood_oxygen_spo2: f32,
    pub galvanic_skin_u_siemens: f32,
    pub core_temp_celsius: f32,
    pub stress_index: f32,
}

impl Default for BiometricVitals {
    fn default() -> Self {
        Self {
            heart_rate_bpm: 72.0,
            blood_oxygen_spo2: 98.5,
            galvanic_skin_u_siemens: 4.0,
            core_temp_celsius: 36.6,
            stress_index: 0.15,
        }
    }
}

impl BiometricVitals {
    pub fn is_safe(&self) -> bool {
        (45.0..=175.0).contains(&self.heart_rate_bpm)
            && self.blood_oxygen_spo2 >= 92.0
            && (35.0..=38.5).contains(&self.core_temp_celsius)
            && self.stress_index <= 0.85
    }
}

/// Watchdog biométrico: monitorea signos vitales y dispara protocolo de parada.
#[derive(Debug, Clone)]
pub struct BiotelemetryWatchdog {
    consecutive_unsafe_ms: u32,
    max_unsafe_duration_ms: u32,
    emergency_triggered: bool,
}

impl Default for BiotelemetryWatchdog {
    fn default() -> Self {
        Self {
            consecutive_unsafe_ms: 0,
            max_unsafe_duration_ms: 500, // 500 ms fuera de rango activa fail-safe
            emergency_triggered: false,
        }
    }
}

impl BiotelemetryWatchdog {
    pub fn new(max_unsafe_duration_ms: u32) -> Self {
        Self {
            consecutive_unsafe_ms: 0,
            max_unsafe_duration_ms: max_unsafe_duration_ms.max(50),
            emergency_triggered: false,
        }
    }

    pub fn inspect(&mut self, vitals: &BiometricVitals, dt_ms: u32) -> bool {
        if vitals.is_safe() {
            self.consecutive_unsafe_ms = 0;
            self.emergency_triggered = false;
            false
        } else {
            self.consecutive_unsafe_ms = self.consecutive_unsafe_ms.saturating_add(dt_ms);
            if self.consecutive_unsafe_ms >= self.max_unsafe_duration_ms {
                self.emergency_triggered = true;
            }
            self.emergency_triggered
        }
    }

    pub fn is_emergency(&self) -> bool {
        self.emergency_triggered
    }
}

/// Paquete de retroalimentación somatosensorial (FullDive Feedback).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SomatosensoryPacket {
    pub channel_id: u16,
    pub frequency_hz: f32,
    pub current_microamps: f32,
    pub duration_ms: u16,
    pub thermal_delta_celsius: f32,
}

impl Default for SomatosensoryPacket {
    fn default() -> Self {
        Self {
            channel_id: 0,
            frequency_hz: 50.0,
            current_microamps: 10.0,
            duration_ms: 20,
            thermal_delta_celsius: 0.0,
        }
    }
}

/// Limitador estricto de estimulación somatosensorial y detector de resonancia cortical.
#[derive(Debug, Clone)]
pub struct SomatosensoryLimiter {
    pub max_current_microamps: f32,
    pub max_frequency_hz: f32,
    pub max_duration_ms: u16,
    pub max_thermal_delta: f32,
    last_motor_rhythm_hz: f32,
}

impl Default for SomatosensoryLimiter {
    fn default() -> Self {
        Self {
            max_current_microamps: 50.0,
            max_frequency_hz: 250.0,
            max_duration_ms: 50,
            max_thermal_delta: 2.0,
            last_motor_rhythm_hz: 0.0,
        }
    }
}

impl SomatosensoryLimiter {
    pub fn update_motor_rhythm(&mut self, rhythm_hz: f32) {
        self.last_motor_rhythm_hz = rhythm_hz;
    }

    /// Filtra y acota el paquete somatosensorial con garantías físicas.
    pub fn sanitize(&self, mut pkt: SomatosensoryPacket) -> SomatosensoryPacket {
        // Clamp de corriente dura
        pkt.current_microamps = pkt.current_microamps.clamp(0.0, self.max_current_microamps);
        // Clamp de frecuencia
        pkt.frequency_hz = pkt.frequency_hz.clamp(1.0, self.max_frequency_hz);
        // Clamp de duración
        pkt.duration_ms = pkt.duration_ms.min(self.max_duration_ms);
        // Clamp térmico
        pkt.thermal_delta_celsius = pkt
            .thermal_delta_celsius
            .clamp(-self.max_thermal_delta, self.max_thermal_delta);

        // Prevención de bucle de resonancia cortical: si la estimulación coincide
        // con la frecuencia motora con una ventana estrecha, atenuar al 20%
        if self.last_motor_rhythm_hz > 1.0 {
            let diff = (pkt.frequency_hz - self.last_motor_rhythm_hz).abs();
            if diff < 1.5 {
                pkt.current_microamps *= 0.20; // Amortiguación inmediata anti-resonancia
            }
        }
        pkt
    }
}

/// Controlador de desconexión gradual (E-Stop suave para evitar choque sensorial).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetachmentStatus {
    Connected,
    GracefulDisengaging { remaining_ms: u32 },
    SafelyDisconnected,
}

#[derive(Debug, Clone)]
pub struct GradualDetachmentController {
    status: DetachmentStatus,
    fade_duration_ms: u32,
}

impl Default for GradualDetachmentController {
    fn default() -> Self {
        Self {
            status: DetachmentStatus::Connected,
            fade_duration_ms: 300,
        }
    }
}

impl GradualDetachmentController {
    pub fn new(fade_duration_ms: u32) -> Self {
        Self {
            status: DetachmentStatus::Connected,
            fade_duration_ms: fade_duration_ms.max(50),
        }
    }

    pub fn trigger_emergency(&mut self) {
        if self.status == DetachmentStatus::Connected {
            self.status = DetachmentStatus::GracefulDisengaging {
                remaining_ms: self.fade_duration_ms,
            };
        }
    }

    pub fn update(&mut self, dt_ms: u32) -> f32 {
        match self.status {
            DetachmentStatus::Connected => 1.0,
            DetachmentStatus::GracefulDisengaging {
                ref mut remaining_ms,
            } => {
                if *remaining_ms <= dt_ms {
                    self.status = DetachmentStatus::SafelyDisconnected;
                    0.0
                } else {
                    *remaining_ms -= dt_ms;
                    (*remaining_ms as f32 / self.fade_duration_ms as f32).clamp(0.0, 1.0)
                }
            }
            DetachmentStatus::SafelyDisconnected => 0.0,
        }
    }

    pub fn status(&self) -> DetachmentStatus {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(values: &[f32], channels: usize, sample_rate: u32) -> NeuralBatch {
        let mut b = NeuralBatch::with_capacity(channels, values.len() / channels.max(1));
        for (i, v) in values.iter().enumerate() {
            b.samples[i] = *v;
        }
        b.len = values.len() / channels.max(1);
        b.channels = channels;
        b.sample_rate = sample_rate;
        b.t0_ns = 1_000;
        b
    }

    #[test]
    fn biquad_impulse_is_bounded() {
        let mut f = Biquad::bandpass(1_000.0, 12.0, 1.2);
        let mut peak = 0.0f32;
        for i in 0..20_000 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = f.process(x);
            assert!(y.is_finite());
            peak = peak.max(y.abs());
        }
        assert!(peak < 10.0, "peak={peak}");
    }

    #[test]
    fn non_finite_input_is_recovered() {
        let mut f = Biquad::notch(1_000.0, 50.0, 30.0);
        let _ = f.process(f32::NAN);
        assert!(f.process(0.0).is_finite());
    }

    #[test]
    fn decoder_marks_artifacts() {
        let mut d = Decoder::new(2, 1_000);
        let mut b = batch(&[f32::NAN, 0.0, 0.0, 0.0], 2, 1_000);
        b.len = 2;
        let f = d.push(&b);
        assert_eq!(f.flags & flags::ARTIFACT, flags::ARTIFACT);
        assert!(!f.is_valid());
        assert_eq!(d.drain_events().len(), 1);
    }

    #[test]
    fn decoder_is_calibrating_until_window_completes() {
        let mut d = Decoder::with_calibration(2, 1_000, 100);
        let b = batch(&[0.0; 20], 2, 1_000); // 10 muestras
        let f = d.push(&b);
        assert_eq!(f.flags & flags::CALIBRATING, flags::CALIBRATING);
        assert!(!f.is_valid());
        assert!(d.calibration_fraction() > 0.0);
    }

    #[test]
    fn decoder_rejects_sample_rate_mismatch() {
        let mut d = Decoder::new(2, 1_000);
        let b = batch(&[0.0; 20], 2, 500);
        let f = d.push(&b);
        assert_eq!(f.flags & flags::ARTIFACT, flags::ARTIFACT);
    }

    #[test]
    fn config_validation_rejects_bad_ranges() {
        let c = Config {
            channels: 0,
            ..Config::default()
        };
        assert!(c.validate().is_err());
        let c = Config {
            sample_rate: 10,
            ..Config::default()
        };
        assert!(c.validate().is_err());
        let c = Config {
            rt_priority: 120,
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn disabled_fulldive_stays_neutral_without_thread() {
        let c = Config {
            enabled: false,
            ..Config::default()
        };
        let mut handle = FullDiveHandle::spawn(c).expect("handle deshabilitado");
        let later = Instant::now() + WATCHDOG + Duration::from_millis(1);
        let frame = handle.snapshot(later);
        assert!(!frame.is_valid());
        assert!(frame.axes.iter().all(|v| *v == 0.0));
        assert!(!handle.health().running);
        assert!(handle.next_event().is_none());
    }

    #[test]
    fn replay_roundtrip_is_deterministic() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "tlaxtrelom-replay-{}-{}.tlxneuro",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        let samples = vec![0.5f32, -0.5, 1.0, -1.0, 0.25, -0.25];
        write_replay_log(&path, 2, 1_000, &samples).unwrap();

        let mut s = ReplaySource::open(&path).unwrap();
        let mut out = NeuralBatch::with_capacity(2, 8);
        assert_eq!(s.poll(&mut out).unwrap(), 3);
        assert_eq!(out.samples[..6], samples[..]);
        assert_eq!(s.poll(&mut out).unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn replay_rejects_bad_magic() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("tlaxtrelom-bad-{}.tlxneuro", std::process::id()));
        std::fs::write(&path, b"NOTVALID").unwrap();
        assert!(ReplaySource::open(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn intent_frame_layout_is_cache_line_sized() {
        assert_eq!(std::mem::size_of::<IntentFrame>(), 56);
        assert_eq!(std::mem::align_of::<IntentFrame>(), 8);
    }

    #[test]
    fn biotelemetry_watchdog_detects_sustained_unsafe_vitals() {
        let mut watchdog = BiotelemetryWatchdog::new(300);
        let safe = BiometricVitals::default();
        assert!(!watchdog.inspect(&safe, 100));

        let unsafe_vitals = BiometricVitals {
            heart_rate_bpm: 195.0, // Taquicardia severa
            ..BiometricVitals::default()
        };
        assert!(!watchdog.inspect(&unsafe_vitals, 100)); // 100 ms < 300 ms
        assert!(!watchdog.inspect(&unsafe_vitals, 150)); // 250 ms < 300 ms
        assert!(watchdog.inspect(&unsafe_vitals, 60)); // 310 ms >= 300 ms => trigger
        assert!(watchdog.is_emergency());
    }

    #[test]
    fn somatosensory_limiter_clamps_unsafe_current_and_resonance() {
        let mut limiter = SomatosensoryLimiter::default();
        limiter.update_motor_rhythm(20.0); // Ritmo motor a 20 Hz (beta)

        let unsafe_pkt = SomatosensoryPacket {
            channel_id: 1,
            frequency_hz: 20.2,         // Muy cerca de 20.0 Hz => resonancia
            current_microamps: 100.0,   // Excede 50 µA
            duration_ms: 100,           // Excede 50 ms
            thermal_delta_celsius: 5.0, // Excede 2.0 °C
        };

        let sanitized = limiter.sanitize(unsafe_pkt);
        // Debe acotar duración a 50 ms y térmica a 2.0 °C
        assert_eq!(sanitized.duration_ms, 50);
        assert_eq!(sanitized.thermal_delta_celsius, 2.0);
        // La corriente máxima es 50 µA, y al haber resonancia (< 1.5 Hz de 20.0), se atenúa al 20%: 50 * 0.2 = 10 µA
        assert!(
            (sanitized.current_microamps - 10.0).abs() < 1e-4,
            "corriente={}",
            sanitized.current_microamps
        );
    }

    #[test]
    fn gradual_detachment_fades_smoothly() {
        let mut c = GradualDetachmentController::new(300);
        assert_eq!(c.update(50), 1.0);

        c.trigger_emergency();
        let f1 = c.update(150);
        assert!((f1 - 0.5).abs() < 1e-3, "f1={f1}");

        let f2 = c.update(160);
        assert_eq!(f2, 0.0);
        assert_eq!(c.status(), DetachmentStatus::SafelyDisconnected);
    }
}
