// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
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

use crate::coords::WorldOrigin;
use crate::xr::StereoFrame;

/// Versión mayor de la ABI C de geometría. Debe coincidir con
/// `tlaxtrelom_geo_abi_version()` de la isla C++.
///
/// 2 añadió `View::origin` (origen flotante) y el `origin[3]` en `double` de la
/// cabecera de asset; con él el LOD se calcula en `double` y el streamer hace
/// frustum culling.
pub const GEO_ABI_VERSION: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub view_proj: [f32; 16],
    pub camera: [f32; 3],
    pub fov_y_rad: f32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub pixel_error_budget: f32,
    /// Dónde está el `(0,0,0)` de `camera` en coordenadas globales, en metros.
    /// Los centers del asset son relativos a su propio origen de sector; la
    /// distancia de LOD es `(asset_origin + center) - (origin + camera)` en
    /// `double`. Sin este campo el mundo grande colapsa el LOD (ver
    /// [`crate::coords`]).
    pub origin: [f64; 3],
}

impl View {
    /// Construye la vista que consume el streamer a partir del frame XR y del
    /// origen flotante del mundo. Se usa el ojo izquierdo como referencia de
    /// culling: el derecho está a menos de 4 cm, por debajo del umbral de error
    /// de la jerarquía.
    pub fn from_frame(frame: &StereoFrame, width: u32, height: u32, origin: &WorldOrigin) -> Self {
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
            origin: origin.anchor,
        }
    }

    /// Vista neutra para tests y para el selftest sin GPU: cámara en el origen
    /// local mirando a +Z con perspectiva de 90°.
    ///
    /// Deliberadamente no es una identidad. El streamer deriva el frustum de
    /// `view_proj`, y una matriz identidad significaría "el mundo visible es el
    /// cubo NDC unitario": podaría hasta el último cluster cocinado en metros.
    pub fn neutral() -> Self {
        let camera = [0.0, crate::world::EYE_HEIGHT, 0.0];
        let view_proj = crate::xr::multiply(
            &crate::xr::perspective(90.0f32.to_radians(), 1.0, 0.05, 10_000.0),
            &crate::xr::view_matrix(0.0, camera),
        );
        Self {
            view_proj,
            camera,
            fov_y_rad: 90.0f32.to_radians(),
            screen_width: 1,
            screen_height: 1,
            pixel_error_budget: 1.0,
            origin: [0.0; 3],
        }
    }

    /// Una vista corrupta (NaN/inf) debe detectarse antes de cruzar la ABI.
    pub fn is_finite(&self) -> bool {
        self.view_proj.iter().all(|v| v.is_finite())
            && self.camera.iter().all(|v| v.is_finite())
            && self.origin.iter().all(|v| v.is_finite())
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
        assert_eq!(std::mem::size_of::<View>(), 120);
        // El `origin` `f64` va al final, tras 4 bytes de relleno por alineación.
        assert_eq!(std::mem::offset_of!(View, origin), 96);
        assert_eq!(std::mem::size_of::<PageRequest>(), 24);
        assert_eq!(std::mem::size_of::<GeometryStats>(), 64);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<ReadyPage>(), 32);
    }

    #[test]
    fn neutral_view_is_finite_and_has_a_real_frustum() {
        let v = View::neutral();
        assert!(v.is_finite());
        // Una identidad podaría todo: el frustum debe venir de una proyección,
        // y la firma de una perspectiva es que `w` salga de `z`.
        assert_eq!(v.view_proj[11], 1.0, "view_proj sin divide en perspectiva");
    }

    #[test]
    fn origin_travels_with_the_view() {
        let frame = StereoFrame::mono(
            crate::xr::HeadPose {
                position: [0.0, 1.7, 0.0],
                yaw: 0.0,
            },
            1.0,
            90.0f32.to_radians(),
            0,
            std::time::Instant::now(),
        );
        let origin = WorldOrigin::new([1e11, 0.0, -4e10]);
        let view = View::from_frame(&frame, 1920, 1080, &origin);
        assert_eq!(view.origin, origin.anchor);
        assert!(view.is_finite());
    }

    #[test]
    fn corrupt_view_is_rejected() {
        let mut v = View::neutral();
        v.camera[1] = f32::NAN;
        assert!(!v.is_finite());
        // Un anclaje basura tampoco cruza: corrompería cada distancia del DAG.
        let mut v = View::neutral();
        v.origin[2] = f64::NAN;
        assert!(!v.is_finite());
    }

    #[test]
    fn bridge_accepts_frames_and_reports_stats() {
        let mut bridge = GeometryBridge::new().expect("bridge");
        assert_eq!(bridge.abi_version(), GEO_ABI_VERSION);
        bridge
            .begin_frame(&View::neutral(), 7)
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

    /// Cocinador ↔ streamer ↔ puente: sin esto, un desacuerdo de layout en la
    /// cabecera TLXG se vería primero como una escena vacía en runtime.
    #[cfg(feature = "native-geometry")]
    #[test]
    fn the_cooked_demo_asset_streams_a_page() {
        use std::ffi::CString;

        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/demo_cluster.tlxg");
        assert!(
            path.exists(),
            "falta {}; generarlo con `make -C native/tlaxtrelom-geometry cook`",
            path.display()
        );

        let mut bridge = GeometryBridge::new().expect("bridge");
        let c_path = CString::new(path.to_str().expect("ruta UTF-8")).expect("sin NUL interno");
        bridge
            .register_asset(1, &c_path)
            .expect("el asset cocinado debería cargar con la ABI actual");

        // El cluster de demo flota a 12 m hacia +Z de la cámara neutra.
        let mut pages = 0usize;
        for frame in 0..64 {
            bridge.begin_frame(&View::neutral(), frame).expect("frame");
            let mut requests = [PageRequest::ZERO; 16];
            let _ = bridge.collect(&mut requests);
            let mut ready = [ReadyPage::ZERO; 8];
            let count = bridge.drain_ready(&mut ready);
            for page in &ready[..count] {
                // SAFETY: el puntero es válido hasta `ack_upload`.
                assert!(!unsafe { page.as_slice() }.is_empty());
                pages += 1;
                bridge.ack_upload(page.page_id).expect("ack");
            }
            if pages > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(pages > 0, "el asset no produjo ninguna página");
    }
}
