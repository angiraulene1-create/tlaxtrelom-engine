// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Proyección multidimensional (2D, 2.5D, 3D, 3.5D, 4D, 4.5D y 5D).
//!
//! La regla es explícita: **no hay física nueva ni dimensiones mágicas**. Lo
//! que hay es un pipeline de proyección matemáticamente definido:
//!
//! 1. Un punto de `D` dimensiones se rota con rotaciones de Givens en los
//!    planos `(i, j)` (hasta 10 planos en 5D).
//! 2. Las dimensiones ocultas (`>= 3`) se colapsan en una profundidad de
//!    perspectiva; cuanto más lejos está el punto en esas dimensiones, más
//!    pequeño se proyecta.
//! 3. El resultado es un punto 3D que el pipeline normal de cámara/XR dibuja.
//!
//! `3.5D` y `4.5D` son interpolaciones, no dimensiones fraccionarias: `3.5D`
//! mezcla la cámara ortográfica del `2.5D` con la perspectiva del `3D`, y
//! `4.5D` mezcla la proyección 3D con la 4D del mismo objeto. En ambos casos
//! `blend` (0 = el modo simple, 1 = el modo complejo) fija dónde se está del
//! espectro, y así se documenta.

use anyhow::{bail, Result};

use crate::mesh::MeshVertex;

/// Número máximo de ejes que maneja el proyector.
pub const MAX_AXES: usize = 5;

/// Planos de rotación en orden determinista. Para `D` dimensiones solo se
/// aplican los que cumplen `i < D && j < D`.
pub const PLANES: [(usize, usize); 10] = [
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (1, 2),
    (1, 3),
    (1, 4),
    (2, 3),
    (2, 4),
    (3, 4),
];

/// Modo de presentación dimensional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimensionMode {
    /// 2D plano: la escena se dibuja en coordenadas NDC, sin cámara 3D.
    Flat2D,
    /// 2.5D: mundo 3D con cámara ortográfica inclinada (isométrico).
    Isometric25D,
    /// 3D en perspectiva (modo por defecto).
    #[default]
    Spatial3D,
    /// 3.5D: encuadre isométrico con perspectiva parcial (mezcla 2.5D↔3D).
    ///
    /// No añade una cuarta dimensión: mezcla la matriz de cámara ortográfica
    /// del `2.5D` con su versión en perspectiva. Vive en `xr.rs`, no en el
    /// proyector N-D, porque lo que interpola es la cámara.
    Fractional35D,
    /// 4D: hipercubo proyectado a 3D.
    Hyper4D,
    /// 4.5D: morph continuo entre la vista 3D y la 4D.
    Fractional45D,
    /// 5D: hipercubo de 5 dimensiones proyectado a 3D.
    Penteract5D,
}

impl DimensionMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "2d" | "2" | "flat" | "plano" => Some(Self::Flat2D),
            "2.5d" | "2_5d" | "2-5d" | "iso" | "isometric" => Some(Self::Isometric25D),
            "3d" | "3" | "spatial" | "espacial" => Some(Self::Spatial3D),
            "3.5d" | "3_5d" | "3-5d" | "depth" | "profundidad" => Some(Self::Fractional35D),
            "4d" | "4" | "hyper" | "hiper" => Some(Self::Hyper4D),
            "4.5d" | "4_5d" | "4-5d" | "fractional" | "fraccional" => Some(Self::Fractional45D),
            "5d" | "5" | "penteract" | "pentatopo" => Some(Self::Penteract5D),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Flat2D => "2d",
            Self::Isometric25D => "2.5d",
            Self::Spatial3D => "3d",
            Self::Fractional35D => "3.5d",
            Self::Hyper4D => "4d",
            Self::Fractional45D => "4.5d",
            Self::Penteract5D => "5d",
        }
    }

    /// Dimensión del espacio de entrada que consume el proyector.
    pub fn spatial_dimension(self) -> usize {
        match self {
            Self::Flat2D => 2,
            Self::Isometric25D | Self::Spatial3D | Self::Fractional35D => 3,
            Self::Hyper4D | Self::Fractional45D => 4,
            Self::Penteract5D => 5,
        }
    }

    /// Número de ejes que se colapsan en profundidad de perspectiva.
    pub fn extra_axes(self) -> usize {
        self.spatial_dimension().saturating_sub(3)
    }

    /// `true` si la escena se dibuja con la cámara ortográfica plana.
    ///
    /// `3.5D` no entra aquí: su cámara es una mezcla, y a `blend = 0` coincide
    /// con la ortográfica pero a `blend = 1` es perspectiva plena.
    pub fn is_flat(self) -> bool {
        matches!(self, Self::Flat2D)
    }

    /// `true` si la cámara usa proyección ortográfica 3D.
    pub fn uses_ortho(self) -> bool {
        matches!(self, Self::Flat2D | Self::Isometric25D)
    }

    /// `true` si hay que generar el politopo hiperdimensional.
    pub fn needs_hyper_scene(self) -> bool {
        matches!(
            self,
            Self::Hyper4D | Self::Fractional45D | Self::Penteract5D
        )
    }

    /// Valor que viaja al shader de presentación para adaptar el fondo.
    pub fn shader_code(self) -> f32 {
        match self {
            Self::Flat2D => 2.0,
            Self::Isometric25D => 2.5,
            Self::Spatial3D => 3.0,
            Self::Fractional35D => 3.5,
            Self::Hyper4D => 4.0,
            Self::Fractional45D => 4.5,
            Self::Penteract5D => 5.0,
        }
    }

    /// Ángulo de inclinación de cámara (radianes) de los modos isométricos.
    pub fn camera_pitch(self) -> f32 {
        match self {
            Self::Isometric25D | Self::Fractional35D => -35.0_f32.to_radians(),
            _ => 0.0,
        }
    }
}

/// Configuración del proyector multidimensional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionConfig {
    pub mode: DimensionMode,
    /// Distancia focal de la proyección de dimensiones ocultas. Valores
    /// pequeños exageran el efecto; grandes lo acercan a una proyección
    /// ortográfica.
    pub focal_distance: f32,
    /// Mezcla 3D↔4D para `Fractional45D` (0 = 3D, 1 = 4D).
    pub blend: f32,
    /// Grosor de las aristas del politopo (unidades del mundo).
    pub edge_thickness: f32,
    /// Rotaciones base por plano, en radianes. La animación suma encima.
    pub angles: [f32; PLANES.len()],
}

impl Default for DimensionConfig {
    fn default() -> Self {
        Self {
            mode: DimensionMode::Spatial3D,
            focal_distance: 6.0,
            blend: 0.5,
            edge_thickness: 0.045,
            angles: [0.0; PLANES.len()],
        }
    }
}

impl DimensionConfig {
    pub fn validate(&self) -> Result<()> {
        if !(0.1..=1_000.0).contains(&self.focal_distance) || !self.focal_distance.is_finite() {
            bail!(
                "focal_distance fuera de [0.1, 1000]: {}",
                self.focal_distance
            );
        }
        if !(0.0..=1.0).contains(&self.blend) || !self.blend.is_finite() {
            bail!("dimension_blend fuera de [0, 1]: {}", self.blend);
        }
        if !(0.001..=1.0).contains(&self.edge_thickness) || !self.edge_thickness.is_finite() {
            bail!(
                "edge_thickness fuera de [0.001, 1.0]: {}",
                self.edge_thickness
            );
        }
        if self.angles.iter().any(|a| !a.is_finite()) {
            bail!("los ángulos de dimensión deben ser finitos");
        }
        Ok(())
    }

    /// Copia la configuración con las rotaciones animadas para `time_s`.
    pub fn animated(self, time_s: f32) -> Self {
        if !time_s.is_finite() {
            return self;
        }
        let mut out = self;
        let t = time_s.rem_euclid(std::f32::consts::TAU);
        let dim = out.mode.spatial_dimension();
        // Velocidades distintas por plano para que el politopo no parezca una
        // rotación rígida y se aprecie la estructura de las dimensiones extra.
        let speeds = [0.20, 0.35, 0.70, 1.10, 0.28, 0.90, 1.30, 0.50, 1.05, 0.75];
        for (idx, (i, j)) in PLANES.iter().enumerate() {
            if *i < dim && *j < dim {
                out.angles[idx] = wrap_angle(out.angles[idx] + t * speeds[idx]);
            }
        }
        out
    }
}

/// Punto de hasta 5 dimensiones. Los ejes no usados valen 0.
pub type NdPoint = [f32; MAX_AXES];

/// Aplica rotaciones de Givens en los planos `(i, j)` activos.
pub fn rotate_nd(point: NdPoint, angles: &[f32; PLANES.len()], dim: usize) -> NdPoint {
    let mut out = point;
    let dim = dim.clamp(1, MAX_AXES);
    for (idx, (i, j)) in PLANES.iter().enumerate() {
        if *i >= dim || *j >= dim {
            continue;
        }
        let angle = angles[idx];
        if !angle.is_finite() {
            continue;
        }
        let (s, c) = angle.sin_cos();
        let xi = out[*i];
        let xj = out[*j];
        out[*i] = c * xi - s * xj;
        out[*j] = s * xi + c * xj;
    }
    out
}

/// Proyecta un punto N-D a 3D. Devuelve `None` si la entrada o la salida no
/// son finitas, o si el punto cae en el plano de proyección.
pub fn project(point: NdPoint, cfg: &DimensionConfig) -> Option<[f32; 3]> {
    let dim = cfg.mode.spatial_dimension();
    if !(2..=MAX_AXES).contains(&dim) {
        return None;
    }
    if point[..dim].iter().any(|v| !v.is_finite()) {
        return None;
    }
    if !cfg.focal_distance.is_finite() || cfg.focal_distance <= 0.0 {
        return None;
    }

    let rotated = rotate_nd(point, &cfg.angles, dim);

    let projected = match cfg.mode {
        DimensionMode::Flat2D => [rotated[0], rotated[1], 0.0],
        DimensionMode::Isometric25D | DimensionMode::Spatial3D | DimensionMode::Fractional35D => {
            [rotated[0], rotated[1], rotated[2]]
        }
        DimensionMode::Hyper4D => project_hidden(&rotated, 3, dim, cfg.focal_distance)?,
        DimensionMode::Penteract5D => project_hidden(&rotated, 3, dim, cfg.focal_distance)?,
        DimensionMode::Fractional45D => {
            let p3 = [rotated[0], rotated[1], rotated[2]];
            let p4 = project_hidden(&rotated, 3, dim, cfg.focal_distance)?;
            let t = cfg.blend.clamp(0.0, 1.0);
            [
                p3[0] + (p4[0] - p3[0]) * t,
                p3[1] + (p4[1] - p3[1]) * t,
                p3[2] + (p4[2] - p3[2]) * t,
            ]
        }
    };

    if projected.iter().all(|v| v.is_finite()) {
        Some(projected)
    } else {
        None
    }
}

/// Colapsa los ejes `first_hidden..dim` en una profundidad de perspectiva y
/// conserva los tres primeros ejes como coordenadas 3D.
fn project_hidden(p: &NdPoint, first_hidden: usize, dim: usize, focal: f32) -> Option<[f32; 3]> {
    let hidden = dim.checked_sub(first_hidden)?;
    if hidden == 0 {
        return Some([p[0], p[1], p[2]]);
    }
    let weight = 1.0 / hidden as f32;
    let depth: f32 = p[first_hidden..dim].iter().map(|v| v * weight).sum();
    let denom = focal + depth;
    if !denom.is_finite() || denom <= 1e-4 {
        return None;
    }
    let scale = focal / denom;
    let out = [p[0] * scale, p[1] * scale, p[2] * scale];
    if out.iter().all(|v| v.is_finite()) {
        Some(out)
    } else {
        None
    }
}

/// Politopo de hipercubo en `dim` dimensiones (2..=5).
#[derive(Debug, Clone)]
pub struct NdMesh {
    pub vertices: Vec<NdPoint>,
    pub edges: Vec<(u16, u16)>,
}

impl NdMesh {
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// Genera el hipercubo `[-1, 1]^dim` y sus aristas.
pub fn hypercube(dim: usize) -> NdMesh {
    let dim = dim.clamp(1, MAX_AXES);
    let count = 1usize << dim;
    let mut vertices = Vec::with_capacity(count);
    for i in 0..count {
        let mut p = [0.0f32; MAX_AXES];
        for (k, slot) in p.iter_mut().enumerate().take(dim) {
            *slot = if (i >> k) & 1 == 1 { 1.0 } else { -1.0 };
        }
        vertices.push(p);
    }

    let mut edges = Vec::new();
    for i in 0..count {
        for k in 0..dim {
            let j = i ^ (1 << k);
            if j > i {
                edges.push((i as u16, j as u16));
            }
        }
    }
    NdMesh { vertices, edges }
}

/// Proyecta las aristas del politopo y las convierte en prismas finos para que
/// el pipeline de triángulos existente las dibuje con iluminación.
pub fn hyper_scene(
    mesh: &NdMesh,
    cfg: &DimensionConfig,
    offset: [f32; 3],
    scale: f32,
) -> (Vec<MeshVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    if !scale.is_finite() || scale <= 0.0 {
        return (vertices, indices);
    }
    let half = (cfg.edge_thickness * 0.5).max(1e-5);
    let palette = [
        [0.10, 0.75, 1.00],
        [0.20, 1.00, 0.55],
        [1.00, 0.55, 0.15],
        [0.90, 0.25, 0.95],
        [1.00, 0.95, 0.30],
    ];

    for &(a, b) in &mesh.edges {
        let (Some(pa), Some(pb)) = (
            project(mesh.vertices[a as usize], cfg),
            project(mesh.vertices[b as usize], cfg),
        ) else {
            continue;
        };
        let p0 = add(scale3(pa, scale), offset);
        let p1 = add(scale3(pb, scale), offset);
        if !p0.iter().chain(p1.iter()).all(|v| v.is_finite()) {
            continue;
        }
        let dir = sub(p1, p0);
        let len = length(dir);
        if !len.is_finite() || len < 1e-5 {
            continue;
        }
        let axis = scale3(dir, 1.0 / len);
        let helper = if axis[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let side = scale3(normalize(cross(axis, helper)), half);
        let up = scale3(normalize(cross(axis, side)), half);

        let bit = ((a ^ b) as u32).trailing_zeros() as usize;
        let color = palette[bit.min(palette.len() - 1)];

        let c000 = add(add(p0, scale3(side, -1.0)), scale3(up, -1.0));
        let c100 = add(add(p0, side), scale3(up, -1.0));
        let c110 = add(add(p0, side), up);
        let c010 = add(add(p0, scale3(side, -1.0)), up);
        let d000 = add(add(p1, scale3(side, -1.0)), scale3(up, -1.0));
        let d100 = add(add(p1, side), scale3(up, -1.0));
        let d110 = add(add(p1, side), up);
        let d010 = add(add(p1, scale3(side, -1.0)), up);

        let mid = scale3(add(p0, p1), 0.5);
        push_prism_face(
            &mut vertices,
            &mut indices,
            [c000, c100, c110, c010],
            mid,
            color,
        );
        push_prism_face(
            &mut vertices,
            &mut indices,
            [d100, d000, d010, d110],
            mid,
            color,
        );
        push_prism_face(
            &mut vertices,
            &mut indices,
            [c000, c100, d100, d000],
            mid,
            color,
        );
        push_prism_face(
            &mut vertices,
            &mut indices,
            [c100, c110, d110, d100],
            mid,
            color,
        );
        push_prism_face(
            &mut vertices,
            &mut indices,
            [c110, c010, d010, d110],
            mid,
            color,
        );
        push_prism_face(
            &mut vertices,
            &mut indices,
            [c010, c000, d000, d010],
            mid,
            color,
        );
    }

    (vertices, indices)
}

fn push_prism_face(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    pts: [[f32; 3]; 4],
    interior: [f32; 3],
    color: [f32; 3],
) {
    let ab = sub(pts[1], pts[0]);
    let ac = sub(pts[2], pts[0]);
    let mut n = normalize(cross(ab, ac));
    if !n.iter().all(|v| v.is_finite()) || length(n) < 1e-6 {
        return;
    }
    let center = scale3(add(add(pts[0], pts[1]), add(pts[2], pts[3])), 0.25);
    if dot(n, sub(center, interior)) < 0.0 {
        n = scale3(n, -1.0);
    }
    let base = vertices.len() as u16;
    for p in pts {
        vertices.push(MeshVertex::new(p, n, color));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = length(a);
    if len.is_finite() && len > 1e-8 {
        scale3(a, 1.0 / len)
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn wrap_angle(a: f32) -> f32 {
    if !a.is_finite() {
        return 0.0;
    }
    let mut w = a % std::f32::consts::TAU;
    if w > std::f32::consts::PI {
        w -= std::f32::consts::TAU;
    } else if w < -std::f32::consts::PI {
        w += std::f32::consts::TAU;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_parses_and_round_trips() {
        for mode in [
            DimensionMode::Flat2D,
            DimensionMode::Isometric25D,
            DimensionMode::Spatial3D,
            DimensionMode::Fractional35D,
            DimensionMode::Hyper4D,
            DimensionMode::Fractional45D,
            DimensionMode::Penteract5D,
        ] {
            assert_eq!(DimensionMode::parse(mode.label()), Some(mode));
            assert!(mode.spatial_dimension() >= 2);
            assert!(mode.spatial_dimension() <= MAX_AXES);
        }
    }

    #[test]
    fn hypercube_counts_match_the_math() {
        let c4 = hypercube(4);
        assert_eq!(c4.vertex_count(), 16);
        assert_eq!(c4.edge_count(), 32);

        let c5 = hypercube(5);
        assert_eq!(c5.vertex_count(), 32);
        assert_eq!(c5.edge_count(), 80);

        let square = hypercube(2);
        assert_eq!(square.vertex_count(), 4);
        assert_eq!(square.edge_count(), 4);
    }

    #[test]
    fn givens_rotation_preserves_length() {
        let p = [0.7, -1.1, 0.3, 0.9, -0.4];
        let mut angles = [0.0; PLANES.len()];
        for (i, a) in angles.iter_mut().enumerate() {
            *a = 0.17 * (i as f32 + 1.0);
        }
        let r = rotate_nd(p, &angles, 5);
        let len = |v: &[f32; 5]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len(&p) - len(&r)).abs() < 1e-4);
    }

    #[test]
    fn projection_is_finite_for_every_mode() {
        let p = [0.4, -0.7, 0.2, 0.9, -0.3];
        for mode in [
            DimensionMode::Flat2D,
            DimensionMode::Isometric25D,
            DimensionMode::Spatial3D,
            DimensionMode::Fractional35D,
            DimensionMode::Hyper4D,
            DimensionMode::Fractional45D,
            DimensionMode::Penteract5D,
        ] {
            let cfg = DimensionConfig {
                mode,
                angles: [0.3; PLANES.len()],
                ..DimensionConfig::default()
            };
            let out = project(p, &cfg).unwrap_or_else(|| panic!("{mode:?} no proyecta"));
            assert!(out.iter().all(|v| v.is_finite()), "{mode:?}: {out:?}");
            assert_eq!(out, project(p, &cfg).expect("determinista"));
        }
    }

    #[test]
    fn non_finite_input_is_rejected() {
        let cfg = DimensionConfig {
            mode: DimensionMode::Penteract5D,
            ..DimensionConfig::default()
        };
        assert!(project([f32::NAN, 0.0, 0.0, 0.0, 0.0], &cfg).is_none());
        assert!(project([0.0, f32::INFINITY, 0.0, 0.0, 0.0], &cfg).is_none());
    }

    #[test]
    fn flat_2d_ignores_depth() {
        let cfg = DimensionConfig {
            mode: DimensionMode::Flat2D,
            ..DimensionConfig::default()
        };
        let a = project([0.3, 0.4, 0.0, 0.0, 0.0], &cfg).unwrap();
        let b = project([0.3, 0.4, 9.0, 0.0, 0.0], &cfg).unwrap();
        assert_eq!(a, b);
        assert_eq!(a[2], 0.0);
    }

    #[test]
    fn fractional_blend_hits_the_midpoint() {
        let cfg = DimensionConfig {
            mode: DimensionMode::Fractional45D,
            blend: 0.5,
            angles: [0.0; PLANES.len()],
            ..DimensionConfig::default()
        };
        let p = [0.5, -0.5, 0.5, 0.5, 0.0];
        let out = project(p, &cfg).unwrap();
        let p3 = [p[0], p[1], p[2]];
        let p4 = project_hidden(&p, 3, 4, cfg.focal_distance).unwrap();
        for i in 0..3 {
            let mid = p3[i] + (p4[i] - p3[i]) * 0.5;
            assert!((out[i] - mid).abs() < 1e-6);
        }
    }

    #[test]
    fn animated_angles_are_bounded_and_finite() {
        let cfg = DimensionConfig {
            mode: DimensionMode::Penteract5D,
            ..DimensionConfig::default()
        };
        let a = cfg.animated(1.0e9);
        assert!(a.angles.iter().all(|v| v.is_finite()));
        assert!(a
            .angles
            .iter()
            .all(|v| v.abs() <= std::f32::consts::PI + 1e-4));
    }

    #[test]
    fn hyper_scene_has_bounded_valid_geometry() {
        let mesh = hypercube(5);
        let cfg = DimensionConfig {
            mode: DimensionMode::Penteract5D,
            ..DimensionConfig::default()
        }
        .animated(0.7);
        let (v, i) = hyper_scene(&mesh, &cfg, [0.0, 2.0, 8.0], 1.3);
        assert!(!v.is_empty());
        assert!(v.len() <= u16::MAX as usize);
        assert_eq!(i.len() % 3, 0);
        assert!(i.iter().all(|&idx| (idx as usize) < v.len()));
        assert!(v
            .iter()
            .all(|vert| vert.position.iter().all(|x| x.is_finite())));
    }

    #[test]
    fn dimension_config_validates_ranges() {
        let mut cfg = DimensionConfig::default();
        assert!(cfg.validate().is_ok());
        cfg.focal_distance = 0.0;
        assert!(cfg.validate().is_err());
        cfg = DimensionConfig::default();
        cfg.blend = 2.0;
        assert!(cfg.validate().is_err());
        cfg = DimensionConfig::default();
        cfg.angles[3] = f32::NAN;
        assert!(cfg.validate().is_err());
    }
}
