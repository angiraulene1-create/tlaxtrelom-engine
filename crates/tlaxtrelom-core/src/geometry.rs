// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//! Puente FFI hacia `native/tlaxtrelom-geometry` (C++).
//!
//! Contrato:
//! - `GeometryBridge` es `!Send`: vive en el hilo de render.
//! - `begin_frame`/`collect`/`drain_ready`/`ack_upload` se llaman una vez por
//!   frame, no por cluster.
//! - Los punteros de `ReadyPage` son válidos hasta `ack_upload(page_id)`.
//! - C++ nunca toca `wgpu`; Rust sube las páginas a GPU.
//!
//! La feature `native-geometry` enlaza la isla C++. Sin ella, el motor arranca
//! con un stub determinista (útil para CI y para trabajar solo en Rust).
//!
//! La ABI se versiona con `GEO_ABI_VERSION`. Un major distinto se rechaza en
//! `GeometryBridge::new`: cargar una isla incompatible es peor que no cargarla.

use crate::xr::StereoFrame;

/// Versión mayor de la ABI C de geometría. Debe coincidir con
/// `tlaxtrelom_geo_abi_version()` de la isla C++.
pub const GEO_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub view_proj: [f32; 16],
    pub camera: [f32; 3],
    pub fov_y_rad: f32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub pixel_error_budget: f32,
}

impl View {
    /// Construye la vista que consume el streamer a partir del frame XR.
    /// Se usa el ojo izquierdo como referencia de culling: el derecho está a
    /// menos de 4 cm, por debajo del umbral de error de la jerarquía.
    pub fn from_frame(frame: &StereoFrame, width: u32, height: u32) -> Self {
        let eye = frame.left;
        let fov = if eye.view_proj[5].is_finite() && eye.view_proj[5] > 1e-6 {
            2.0 * (1.0 / eye.view_proj[5]).atan()
        } else {
            90.0f32.to_radians()
        };

        Self {
            view_proj: eye.view_proj,
            camera: eye.position,
            fov_y_rad: fov,
            screen_width: width.max(1),
            screen_height: height.max(1),
            // 1 píxel de error: calidad "cinematográfica". Subirlo a 2–4 en
            // VR/móvil reduce el ancho de banda de páginas drásticamente.
            pixel_error_budget: 1.0,
        }
    }

    /// Vista neutra para tests y para el selftest sin GPU.
    pub fn identity() -> Self {
        let mut view_proj = [0.0f32; 16];
        view_proj[0] = 1.0;
        view_proj[5] = 1.0;
        view_proj[10] = 1.0;
        view_proj[15] = 1.0;
        Self {
            view_proj,
            camera: [0.0, 1.7, 0.0],
            fov_y_rad: 90.0f32.to_radians(),
            screen_width: 1,
            screen_height: 1,
            pixel_error_budget: 1.0,
        }
    }

    /// Una vista corrupta (NaN/inf) debe detectarse antes de cruzar la ABI.
    pub fn is_finite(&self) -> bool {
        self.view_proj.iter().all(|v| v.is_finite())
            && self.camera.iter().all(|v| v.is_finite())
            && self.fov_y_rad.is_finite()
            && self.pixel_error_budget.is_finite()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequest {
    pub page_id: u64,
    pub asset_id: u32,
    pub lod: u32,
    pub priority: u32,
    pub _pad: u32,
}

impl PageRequest {
    pub const ZERO: Self = Self {
        page_id: 0,
        asset_id: 0,
        lod: 0,
        priority: 0,
        _pad: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ReadyPage {
    pub page_id: u64,
    pub data: *const std::ffi::c_void,
    pub size: u32,
    pub format: u32,
    pub _pad: u32,
}

impl ReadyPage {
    pub const ZERO: Self = Self {
        page_id: 0,
        data: std::ptr::null(),
        size: 0,
        format: 0,
        _pad: 0,
    };

    /// # Safety
    /// El puntero es válido hasta `tlaxtrelom_geo_ack_upload(page_id)` y el
    /// llamador no debe conservar el slice después de ese punto.
    pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
        if self.data.is_null() || self.size == 0 {
            return &[];
        }
        // SAFETY: contrato del llamador (ver doc de la función).
        unsafe { std::slice::from_raw_parts(self.data as *const u8, self.size as usize) }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeometryStats {
    pub pages_resident: u64,
    pub pages_pending: u64,
    pub bytes_resident: u64,
    pub clusters_visible: u64,
    pub clusters_culled: u64,
    pub page_faults: u64,
    pub io_bytes: u64,
    pub io_queue_depth: u32,
    pub _pad: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementación nativa (C++)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "native-geometry")]
mod native {
    use std::ffi::{c_char, c_void, CStr};
    use std::ptr::NonNull;

    use anyhow::{bail, Context, Result};

    use super::{GeometryStats, PageRequest, ReadyPage, View, GEO_ABI_VERSION};

    #[repr(C)]
    struct NativeConfig {
        max_resident_pages: u32,
        io_threads: u32,
        vram_budget_bytes: u64,
        cache_dir: *const c_char,
        max_ready_pages: u32,
        _pad: u32,
    }

    unsafe extern "C" {
        fn tlaxtrelom_geo_abi_version() -> u32;
        fn tlaxtrelom_geo_create(cfg: *const NativeConfig, out: *mut *mut c_void) -> i32;
        fn tlaxtrelom_geo_destroy(s: *mut c_void);
        fn tlaxtrelom_geo_register_asset(s: *mut c_void, asset_id: u32, path: *const c_char)
            -> i32;
        fn tlaxtrelom_geo_begin_frame(s: *mut c_void, view: *const View, frame_id: u64) -> i32;
        fn tlaxtrelom_geo_collect(s: *mut c_void, out: *mut PageRequest, cap: u32) -> u32;
        fn tlaxtrelom_geo_drain_ready(s: *mut c_void, out: *mut ReadyPage, cap: u32) -> u32;
        fn tlaxtrelom_geo_ack_upload(s: *mut c_void, page_id: u64) -> i32;
        fn tlaxtrelom_geo_get_stats(s: *const c_void, out: *mut GeometryStats);
    }

    pub struct GeometryBridge {
        raw: NonNull<c_void>,
        abi: u32,
    }

    impl GeometryBridge {
        pub fn new() -> Result<Self> {
            // SAFETY: la función no tiene precondiciones y solo lee un entero.
            let abi = unsafe { tlaxtrelom_geo_abi_version() };
            if abi != GEO_ABI_VERSION {
                bail!("ABI de geometría incompatible: runtime={GEO_ABI_VERSION}, isla={abi}");
            }

            let cfg = NativeConfig {
                max_resident_pages: 4096,
                io_threads: 4,
                vram_budget_bytes: 6 << 30, // 6 GiB
                cache_dir: c"/tmp/tlaxtrelom-geo".as_ptr(),
                max_ready_pages: 256,
                _pad: 0,
            };
            let mut raw = std::ptr::null_mut();
            // SAFETY: `cfg` y `raw` son válidos durante la llamada.
            let rc = unsafe { tlaxtrelom_geo_create(&cfg, &mut raw) };
            if rc != 0 {
                bail!("tlaxtrelom_geo_create falló (rc={rc})");
            }
            let raw = NonNull::new(raw).context("tlaxtrelom_geo_create devolvió handle nulo")?;
            Ok(Self { raw, abi })
        }

        /// Versión de ABI reportada por la isla C++.
        pub fn abi_version(&self) -> u32 {
            self.abi
        }

        pub fn register_asset(&mut self, asset_id: u32, path: &CStr) -> Result<()> {
            // SAFETY: el handle es válido y `path` es una C string terminada en NUL.
            let rc = unsafe {
                tlaxtrelom_geo_register_asset(self.raw.as_ptr(), asset_id, path.as_ptr())
            };
            if rc != 0 {
                bail!("tlaxtrelom_geo_register_asset({asset_id}) falló (rc={rc})");
            }
            Ok(())
        }

        pub fn begin_frame(&mut self, view: &View, frame_id: u64) -> Result<()> {
            if !view.is_finite() {
                bail!("vista no finita: se rechaza antes de cruzar la ABI");
            }
            // SAFETY: handle válido; `view` vive durante la llamada.
            let rc = unsafe { tlaxtrelom_geo_begin_frame(self.raw.as_ptr(), view, frame_id) };
            if rc != 0 {
                bail!("tlaxtrelom_geo_begin_frame falló (rc={rc})");
            }
            Ok(())
        }

        /// Devuelve cuántas peticiones se generaron (telemetría/priorización).
        pub fn collect(&mut self, out: &mut [PageRequest]) -> usize {
            // SAFETY: el buffer de salida es válido para `out.len()` elementos.
            let n = unsafe {
                tlaxtrelom_geo_collect(self.raw.as_ptr(), out.as_mut_ptr(), out.len() as u32)
            };
            (n as usize).min(out.len())
        }

        pub fn drain_ready(&mut self, out: &mut [ReadyPage]) -> usize {
            // SAFETY: el buffer de salida es válido para `out.len()` elementos.
            let n = unsafe {
                tlaxtrelom_geo_drain_ready(self.raw.as_ptr(), out.as_mut_ptr(), out.len() as u32)
            };
            (n as usize).min(out.len())
        }

        pub fn ack_upload(&mut self, page_id: u64) -> Result<()> {
            // SAFETY: handle válido; el page_id lo devolvió `drain_ready`.
            let rc = unsafe { tlaxtrelom_geo_ack_upload(self.raw.as_ptr(), page_id) };
            if rc != 0 {
                bail!("tlaxtrelom_geo_ack_upload({page_id}) falló (rc={rc})");
            }
            Ok(())
        }

        pub fn stats(&self) -> GeometryStats {
            let mut out = GeometryStats::default();
            // SAFETY: handle válido y `out` es un struct POD correcto.
            unsafe { tlaxtrelom_geo_get_stats(self.raw.as_ptr(), &mut out) };
            out
        }
    }

    impl Drop for GeometryBridge {
        fn drop(&mut self) {
            // SAFETY: el handle se creó con `create` y se destruye una sola vez.
            unsafe { tlaxtrelom_geo_destroy(self.raw.as_ptr()) };
        }
    }
}

#[cfg(feature = "native-geometry")]
pub use native::GeometryBridge;

// ─────────────────────────────────────────────────────────────────────────────
// Stub sin C++ (CI, desarrollo Rust-only)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(feature = "native-geometry"))]
#[derive(Debug, Default)]
pub struct GeometryBridge {
    frame_id: u64,
}

#[cfg(not(feature = "native-geometry"))]
impl GeometryBridge {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self::default())
    }

    pub fn abi_version(&self) -> u32 {
        GEO_ABI_VERSION
    }

    pub fn register_asset(&mut self, _asset_id: u32, _path: &std::ffi::CStr) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn begin_frame(&mut self, view: &View, frame_id: u64) -> anyhow::Result<()> {
        if !view.is_finite() {
            anyhow::bail!("vista no finita");
        }
        self.frame_id = frame_id;
        Ok(())
    }

    pub fn collect(&mut self, _out: &mut [PageRequest]) -> usize {
        0
    }

    pub fn drain_ready(&mut self, _out: &mut [ReadyPage]) -> usize {
        0
    }

    pub fn ack_upload(&mut self, _page_id: u64) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn stats(&self) -> GeometryStats {
        GeometryStats {
            io_queue_depth: 0,
            ..GeometryStats::default()
        }
    }

    /// Número de frame visto por el stub; útil para asserts de integración.
    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_structs_match_header_layout() {
        // Layouts espejo del header C. Si cambia uno, cambiar el otro y subir
        // GEO_ABI_VERSION.
        assert_eq!(std::mem::size_of::<View>(), 92);
        assert_eq!(std::mem::size_of::<PageRequest>(), 24);
        assert_eq!(std::mem::size_of::<GeometryStats>(), 64);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<ReadyPage>(), 32);
    }

    #[test]
    fn identity_view_is_finite() {
        assert!(View::identity().is_finite());
    }

    #[test]
    fn corrupt_view_is_rejected() {
        let mut v = View::identity();
        v.camera[1] = f32::NAN;
        assert!(!v.is_finite());
    }

    #[test]
    fn bridge_accepts_frames_and_reports_stats() {
        let mut bridge = GeometryBridge::new().expect("bridge");
        assert_eq!(bridge.abi_version(), GEO_ABI_VERSION);
        bridge
            .begin_frame(&View::identity(), 7)
            .expect("begin_frame");
        let mut requests = [PageRequest::ZERO; 8];
        let _ = bridge.collect(&mut requests);
        let mut ready = [ReadyPage::ZERO; 4];
        let _ = bridge.drain_ready(&mut ready);
        let _stats = bridge.stats();
    }

    #[test]
    fn empty_ready_page_is_a_safe_empty_slice() {
        let page = ReadyPage::ZERO;
        // SAFETY: no hay puntero, la función devuelve una slice vacía.
        assert!(unsafe { page.as_slice() }.is_empty());
    }
}
