// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! HAL de plataforma (C11) vista desde Rust.
//!
//! La isla C responde preguntas que Rust también podría responder, pero cuya
//! respuesta correcta depende del SO y del compilador: tamaño de página,
//! memoria física, capacidades SIMD, `sched_setaffinity`, `mlockall`. Tenerlas
//! en C permite reutilizarlas desde el firmware y desde plugins sin duplicar
//! `cfg(target_os)` por todo el runtime.
//!
//! La feature `native-hal` está activa por defecto y compila
//! `native/tlaxtrelom-hal` con el compilador de C del sistema (`cc`). Con
//! `--no-default-features` se usa un stub portable.

use std::ffi::{c_char, CStr};

/// Versión de ABI de la HAL. Debe coincidir con `TLAXTRELOM_HAL_ABI_VERSION`.
pub const HAL_ABI_VERSION: u32 = 1;

/// Nivel SIMD detectado en runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SimdLevel {
    #[default]
    Baseline = 0,
    Sse2OrNeon = 1,
    Avx2 = 2,
    Avx512 = 3,
}

impl SimdLevel {
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Sse2OrNeon,
            2 => Self::Avx2,
            3 => Self::Avx512,
            _ => Self::Baseline,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Sse2OrNeon => "sse2/neon",
            Self::Avx2 => "avx2",
            Self::Avx512 => "avx512",
        }
    }
}

/// Espejo exacto de `TlaxtrelomHalInfo` (104 bytes en LP64).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HalInfo {
    pub abi_version: u32,
    pub page_size: u32,
    pub physical_memory_bytes: u64,
    pub logical_cores: u32,
    pub has_avx2: u32,
    pub has_avx512: u32,
    pub has_neon: u32,
    pub has_realtime: u32,
    pub simd_level: u32,
    pub os: [c_char; 32],
    pub arch: [c_char; 32],
}

impl HalInfo {
    pub fn os(&self) -> String {
        // SAFETY: la HAL garantiza NUL final; el array tiene 32 bytes.
        unsafe { CStr::from_ptr(self.os.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn arch(&self) -> String {
        // SAFETY: idem `os`.
        unsafe { CStr::from_ptr(self.arch.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn simd(&self) -> SimdLevel {
        SimdLevel::from_raw(self.simd_level)
    }

    pub fn physical_memory_gib(&self) -> f64 {
        self.physical_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementación nativa (C11)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "native-hal")]
mod native {
    use super::{HalInfo, SimdLevel, HAL_ABI_VERSION};

    unsafe extern "C" {
        fn tlaxtrelom_hal_abi_version() -> u32;
        fn tlaxtrelom_hal_query(out: *mut HalInfo) -> i32;
        fn tlaxtrelom_hal_monotonic_ns() -> u64;
        fn tlaxtrelom_hal_thread_id() -> u64;
        fn tlaxtrelom_hal_set_thread_affinity(core: u32) -> i32;
        fn tlaxtrelom_hal_set_thread_priority(priority: i32) -> i32;
        fn tlaxtrelom_hal_lock_memory() -> i32;
        fn tlaxtrelom_hal_simd_level() -> u32;
        fn tlaxtrelom_hal_hash64(data: *const u8, len: usize, seed: u64) -> u64;
        fn tlaxtrelom_hal_align_up(bytes: u64, alignment: u32) -> u64;
    }

    /// Consulta capacidades. Falla solo si la ABI no coincide o la HAL
    /// devuelve un error de argumento.
    pub fn query() -> anyhow::Result<HalInfo> {
        // SAFETY: llamada sin precondiciones.
        let abi = unsafe { tlaxtrelom_hal_abi_version() };
        if abi != HAL_ABI_VERSION {
            anyhow::bail!("ABI de HAL incompatible: runtime={HAL_ABI_VERSION}, isla={abi}");
        }

        let mut info = HalInfo::default();
        // SAFETY: `info` es un struct POD válido y alineado.
        let rc = unsafe { tlaxtrelom_hal_query(&mut info) };
        if rc != 0 {
            anyhow::bail!("tlaxtrelom_hal_query falló (rc={rc})");
        }
        Ok(info)
    }

    pub fn monotonic_ns() -> u64 {
        // SAFETY: llamada sin precondiciones.
        unsafe { tlaxtrelom_hal_monotonic_ns() }
    }

    pub fn thread_id() -> u64 {
        // SAFETY: llamada sin precondiciones.
        unsafe { tlaxtrelom_hal_thread_id() }
    }

    pub fn set_thread_affinity(core: u32) -> bool {
        // SAFETY: afecta solo al hilo llamador.
        unsafe { tlaxtrelom_hal_set_thread_affinity(core) == 0 }
    }

    pub fn set_thread_priority(priority: i32) -> bool {
        // SAFETY: afecta solo al hilo llamador.
        unsafe { tlaxtrelom_hal_set_thread_priority(priority) == 0 }
    }

    pub fn lock_memory() -> bool {
        // SAFETY: afecta solo al hilo llamador.
        unsafe { tlaxtrelom_hal_lock_memory() == 0 }
    }

    pub fn simd_level() -> SimdLevel {
        // SAFETY: llamada sin precondiciones.
        SimdLevel::from_raw(unsafe { tlaxtrelom_hal_simd_level() })
    }

    pub fn hash64(data: &[u8], seed: u64) -> u64 {
        // SAFETY: `data` es válido para `data.len()` bytes.
        unsafe { tlaxtrelom_hal_hash64(data.as_ptr(), data.len(), seed) }
    }

    pub fn align_up(bytes: u64, alignment: u32) -> u64 {
        // SAFETY: operación aritmética pura.
        unsafe { tlaxtrelom_hal_align_up(bytes, alignment) }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stub portable (sin compilador de C)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(feature = "native-hal"))]
mod native {
    use std::sync::OnceLock;
    use std::time::Instant;

    use super::{HalInfo, SimdLevel, HAL_ABI_VERSION};

    static START: OnceLock<Instant> = OnceLock::new();

    pub fn query() -> anyhow::Result<HalInfo> {
        let mut info = HalInfo {
            abi_version: HAL_ABI_VERSION,
            page_size: 4096,
            physical_memory_bytes: 0,
            logical_cores: std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1),
            simd_level: 0,
            ..HalInfo::default()
        };
        fill(&mut info.os, std::env::consts::OS);
        fill(&mut info.arch, std::env::consts::ARCH);
        Ok(info)
    }

    fn fill(dst: &mut [std::ffi::c_char; 32], src: &str) {
        for (slot, byte) in dst.iter_mut().zip(src.bytes()) {
            *slot = byte as std::ffi::c_char;
        }
    }

    pub fn monotonic_ns() -> u64 {
        START
            .get_or_init(Instant::now)
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64
    }

    pub fn thread_id() -> u64 {
        // El stub no promete identidad estable entre plataformas.
        0
    }

    pub fn set_thread_affinity(_core: u32) -> bool {
        false
    }

    pub fn set_thread_priority(_priority: i32) -> bool {
        false
    }

    pub fn lock_memory() -> bool {
        false
    }

    pub fn simd_level() -> SimdLevel {
        SimdLevel::Baseline
    }

    pub fn hash64(data: &[u8], seed: u64) -> u64 {
        let mut hash = if seed == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            seed
        };
        for byte in data {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    pub fn align_up(bytes: u64, alignment: u32) -> u64 {
        if alignment == 0 || !alignment.is_power_of_two() {
            return 0;
        }
        match bytes.checked_add(alignment as u64 - 1) {
            Some(v) => v & !(alignment as u64 - 1),
            None => 0,
        }
    }
}

pub use native::{
    align_up, hash64, lock_memory, monotonic_ns, query, set_thread_affinity, set_thread_priority,
    simd_level, thread_id,
};

/// Resumen legible para el arranque y la telemetría.
pub fn describe() -> anyhow::Result<String> {
    let info = query()?;
    Ok(format!(
        "{} {} · {} cores · {:.1} GiB · páginas {} B · simd {} · rt {}",
        info.os(),
        info.arch(),
        info.logical_cores,
        info.physical_memory_gib(),
        info.page_size,
        info.simd().as_str(),
        if info.has_realtime != 0 { "sí" } else { "no" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_reports_consistent_capabilities() {
        let info = query().expect("hal query");
        assert_eq!(info.abi_version, HAL_ABI_VERSION);
        assert!(
            info.page_size.is_power_of_two(),
            "page_size={}",
            info.page_size
        );
        assert!(info.logical_cores >= 1);
        assert!(!info.os().is_empty());
        assert!(!info.arch().is_empty());
        assert!(info.simd_level <= 3);
    }

    #[test]
    fn hal_info_layout_is_stable() {
        assert_eq!(std::mem::size_of::<HalInfo>(), 104);
        assert_eq!(std::mem::align_of::<HalInfo>(), 8);
    }

    #[test]
    fn hash_is_deterministic_and_seed_sensitive() {
        let a = hash64(b"tlaxtrelom", 0);
        let b = hash64(b"tlaxtrelom", 0);
        let c = hash64(b"tlaxtrelom", 1);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn align_up_handles_edge_cases() {
        assert_eq!(align_up(0, 4), 0);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(5, 4), 8);
        assert_eq!(align_up(16, 16), 16);
        assert_eq!(align_up(17, 16), 32);
        assert_eq!(align_up(5, 3), 0, "alineación no potencia de dos");
        assert_eq!(align_up(u64::MAX, 16), 0, "desborde");
    }

    #[test]
    fn monotonic_clock_advances() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
    }

    #[test]
    fn describe_is_human_readable() {
        let text = describe().expect("describe");
        assert!(text.contains("cores"));
    }
}
