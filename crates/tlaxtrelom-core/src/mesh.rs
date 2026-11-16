// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Primitivas de malla compartidas por el renderer y el subsistema de
//! dimensiones.
//!
//! El formato de vértice es deliberadamente compacto (36 bytes) y estable:
//! posición, normal y color. Cualquier pipeline que dibuje triángulos —3D,
//! 2.5D o la proyección de un politopo 4D/5D— usa exactamente este layout.

use bytemuck::{Pod, Zeroable};

/// Vértice estándar para mallas con normales y color por vértice.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

impl MeshVertex {
    pub const fn new(position: [f32; 3], normal: [f32; 3], color: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            color,
        }
    }
}

const _: () = assert!(std::mem::size_of::<MeshVertex>() == 36);

/// Añade un quad en el plano XY (normal +Z). Se usa para la escena 2D.
pub fn push_quad_xy(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    center: [f32; 3],
    size: [f32; 2],
    color: [f32; 3],
) {
    let hx = size[0] * 0.5;
    let hy = size[1] * 0.5;
    let z = center[2];
    let Some(base) = u16::try_from(vertices.len()).ok() else {
        return; // sin espacio en los índices u16: mejor omitir que corromper
    };
    let normal = [0.0, 0.0, 1.0];
    for (x, y) in [
        (center[0] - hx, center[1] - hy),
        (center[0] + hx, center[1] - hy),
        (center[0] + hx, center[1] + hy),
        (center[0] - hx, center[1] + hy),
    ] {
        vertices.push(MeshVertex::new([x, y, z], normal, color));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Añade un disco en el plano XY (abanico de triángulos).
pub fn push_disc_xy(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    center: [f32; 3],
    radius: f32,
    segments: u16,
    color: [f32; 3],
) {
    let segments = segments.max(3);
    let normal = [0.0, 0.0, 1.0];
    let Some(base) = u16::try_from(vertices.len()).ok() else {
        return; // sin espacio en los índices u16: mejor omitir que corromper
    };
    vertices.push(MeshVertex::new(center, normal, color));
    for i in 0..=segments {
        let a = std::f32::consts::TAU * (i as f32 / segments as f32);
        vertices.push(MeshVertex::new(
            [
                center[0] + radius * a.cos(),
                center[1] + radius * a.sin(),
                center[2],
            ],
            normal,
            color,
        ));
    }
    for i in 0..segments {
        indices.extend_from_slice(&[base, base + 1 + i, base + 2 + i]);
    }
}

/// Escena 2D de demostración, en coordenadas NDC `[-1, 1]`.
///
/// Es intencionadamente independiente del mundo 3D: el modo `2d` dibuja una
/// vista plana real, sin que el yaw/posición del jugador la deformen.
pub fn create_flat_scene() -> (Vec<MeshVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Rejilla de fondo (líneas finas) en el plano XY. Cada línea cruza el
    // viewport de lado a lado: en estéreo el viewport es la mitad del
    // swapchain, así que ninguna geometría 2D puede salir de [-1, 1]. Las
    // líneas de los bordes se retraen medio grosor para no quedar recortadas
    // a lo largo por la mitad.
    let grid_color = [0.10, 0.20, 0.28];
    let line = 0.0025f32;
    let inset = (1.0 - line * 0.5).max(-1.0);
    for i in -8..=8 {
        let g = (i as f32 / 8.0).clamp(-inset, inset);
        push_quad_xy(
            &mut vertices,
            &mut indices,
            [g, 0.0, 0.85],
            [line, 2.0],
            grid_color,
        );
        push_quad_xy(
            &mut vertices,
            &mut indices,
            [0.0, g, 0.85],
            [2.0, line],
            grid_color,
        );
    }

    // Suelo de la "escena" 2D.
    push_quad_xy(
        &mut vertices,
        &mut indices,
        [0.0, -0.82, 0.75],
        [2.0, 0.36],
        [0.05, 0.12, 0.20],
    );

    // Sol / luna.
    push_disc_xy(
        &mut vertices,
        &mut indices,
        [0.52, 0.46, 0.70],
        0.14,
        32,
        [1.0, 0.62, 0.18],
    );

    // Tres capas con profundidad distinta. En 2D se solapan; en 2.5D el
    // pipeline 3D las separa.
    push_quad_xy(
        &mut vertices,
        &mut indices,
        [-0.42, -0.18, 0.55],
        [0.84, 0.60],
        [0.10, 0.55, 0.90],
    );
    push_quad_xy(
        &mut vertices,
        &mut indices,
        [-0.18, -0.02, 0.35],
        [0.68, 0.52],
        [0.15, 0.85, 0.55],
    );
    push_quad_xy(
        &mut vertices,
        &mut indices,
        [0.12, 0.10, 0.15],
        [0.52, 0.40],
        [0.95, 0.30, 0.55],
    );

    // Barra de "HUD" 2D.
    push_quad_xy(
        &mut vertices,
        &mut indices,
        [0.0, -0.90, 0.05],
        [2.0, 0.035],
        [0.20, 0.70, 0.80],
    );

    (vertices, indices)
}

/// Construye la escena 3D procedural de demostración con suelo, monolito,
/// cubos, pilares, pirámide y ejes de orientación.
pub fn create_demo_scene() -> (Vec<MeshVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // 1. Suelo / Rejilla espacial en Y = 0.0
    let step = 4.0f32;
    let min_x = -24.0f32;
    let max_x = 24.0f32;
    let min_z = -8.0f32;
    let max_z = 36.0f32;

    let mut z = min_z;
    while z < max_z {
        let mut x = min_x;
        while x < max_x {
            let is_runway = x.abs() < 2.0;
            let checker = ((x / step).floor() as i32 + (z / step).floor() as i32) % 2 == 0;
            let color = if is_runway {
                [0.10, 0.45, 0.55]
            } else if checker {
                [0.05, 0.08, 0.14]
            } else {
                [0.03, 0.05, 0.09]
            };

            let Some(base) = u16::try_from(vertices.len()).ok() else {
                break; // sin espacio en los índices u16: mejor cortar que corromper
            };
            let norm = [0.0, 1.0, 0.0];
            vertices.push(MeshVertex::new([x, 0.0, z], norm, color));
            vertices.push(MeshVertex::new([x + step, 0.0, z], norm, color));
            vertices.push(MeshVertex::new([x + step, 0.0, z + step], norm, color));
            vertices.push(MeshVertex::new([x, 0.0, z + step], norm, color));

            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            x += step;
        }
        z += step;
    }

    // 2. Monolito central de datos a 8 metros
    push_box(
        &mut vertices,
        &mut indices,
        [0.0, 2.0, 8.0],
        [0.9, 4.0, 0.9],
        [0.08, 0.65, 0.85],
    );

    // 3. Cubo flotante naranja a la izquierda
    push_box(
        &mut vertices,
        &mut indices,
        [-3.5, 1.6, 6.0],
        [1.2, 1.2, 1.2],
        [0.95, 0.45, 0.12],
    );

    // 4. Cubo flotante magenta a la derecha
    push_box(
        &mut vertices,
        &mut indices,
        [3.5, 1.6, 6.0],
        [1.2, 1.2, 1.2],
        [0.85, 0.22, 0.88],
    );

    // 5. Pilares guía a los costados del camino
    for z_pos in [4.0, 10.0, 16.0, 22.0] {
        push_box(
            &mut vertices,
            &mut indices,
            [-2.2, 0.8, z_pos],
            [0.4, 1.6, 0.4],
            [0.25, 0.70, 0.50],
        );
        push_box(
            &mut vertices,
            &mut indices,
            [2.2, 0.8, z_pos],
            [0.4, 1.6, 0.4],
            [0.25, 0.70, 0.50],
        );
    }

    // 6. Pirámide al fondo
    let pyr_cx = 0.0f32;
    let pyr_cy = 0.0f32;
    let pyr_cz = 18.0f32;
    let pyr_h = 3.5f32;
    let pyr_b = 2.0f32;
    let tip = [pyr_cx, pyr_cy + pyr_h, pyr_cz];
    let p0 = [pyr_cx - pyr_b, pyr_cy, pyr_cz - pyr_b];
    let p1 = [pyr_cx + pyr_b, pyr_cy, pyr_cz - pyr_b];
    let p2 = [pyr_cx + pyr_b, pyr_cy, pyr_cz + pyr_b];
    let p3 = [pyr_cx - pyr_b, pyr_cy, pyr_cz + pyr_b];
    let pyr_color = [0.15, 0.85, 0.40];

    push_tri(&mut vertices, &mut indices, tip, p1, p0, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p2, p1, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p3, p2, pyr_color);
    push_tri(&mut vertices, &mut indices, tip, p0, p3, pyr_color);

    // 7. Ejes de orientación espacial cerca del punto de inicio
    push_box(
        &mut vertices,
        &mut indices,
        [0.5, 0.1, 1.2],
        [0.8, 0.08, 0.08],
        [1.0, 0.15, 0.15],
    ); // Eje +X (Rojo)
    push_box(
        &mut vertices,
        &mut indices,
        [0.1, 0.5, 1.2],
        [0.08, 0.8, 0.08],
        [0.15, 1.0, 0.15],
    ); // Eje +Y (Verde)
    push_box(
        &mut vertices,
        &mut indices,
        [0.1, 0.1, 1.6],
        [0.08, 0.08, 0.8],
        [0.20, 0.45, 1.0],
    ); // Eje +Z (Azul / Frente)

    (vertices, indices)
}

pub fn push_box(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    center: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
) {
    let hx = size[0] * 0.5;
    let hy = size[1] * 0.5;
    let hz = size[2] * 0.5;
    let cx = center[0];
    let cy = center[1];
    let cz = center[2];

    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [cx - hx, cy - hy, cz + hz],
                [cx + hx, cy - hy, cz + hz],
                [cx + hx, cy + hy, cz + hz],
                [cx - hx, cy + hy, cz + hz],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [cx + hx, cy - hy, cz - hz],
                [cx - hx, cy - hy, cz - hz],
                [cx - hx, cy + hy, cz - hz],
                [cx + hx, cy + hy, cz - hz],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [cx + hx, cy - hy, cz + hz],
                [cx + hx, cy - hy, cz - hz],
                [cx + hx, cy + hy, cz - hz],
                [cx + hx, cy + hy, cz + hz],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [cx - hx, cy - hy, cz - hz],
                [cx - hx, cy - hy, cz + hz],
                [cx - hx, cy + hy, cz + hz],
                [cx - hx, cy + hy, cz - hz],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [cx - hx, cy + hy, cz + hz],
                [cx + hx, cy + hy, cz + hz],
                [cx + hx, cy + hy, cz - hz],
                [cx - hx, cy + hy, cz - hz],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [cx - hx, cy - hy, cz - hz],
                [cx + hx, cy - hy, cz - hz],
                [cx + hx, cy - hy, cz + hz],
                [cx - hx, cy - hy, cz + hz],
            ],
        ),
    ];

    for (norm, pts) in faces {
        let Some(base) = u16::try_from(vertices.len()).ok() else {
            return; // sin espacio en los índices u16: mejor omitir que corromper
        };
        for pt in pts {
            vertices.push(MeshVertex::new(pt, norm, color));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub fn push_tri(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    color: [f32; 3],
) {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let len = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2])
        .sqrt()
        .max(1e-6);
    let norm = [cross[0] / len, cross[1] / len, cross[2] / len];
    let Some(base) = u16::try_from(vertices.len()).ok() else {
        return; // sin espacio en los índices u16: mejor omitir que corromper
    };
    vertices.push(MeshVertex::new(a, norm, color));
    vertices.push(MeshVertex::new(b, norm, color));
    vertices.push(MeshVertex::new(c, norm, color));
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_is_shader_compatible() {
        assert_eq!(std::mem::size_of::<MeshVertex>(), 36);
        assert_eq!(std::mem::align_of::<MeshVertex>(), 4);
    }

    #[test]
    fn demo_scene_has_valid_indices() {
        let (vertices, indices) = create_demo_scene();
        assert!(!vertices.is_empty());
        assert!(vertices.len() <= u16::MAX as usize);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        assert_eq!(indices.len() % 3, 0);
    }

    #[test]
    fn flat_scene_stays_in_ndc_and_has_valid_indices() {
        let (vertices, indices) = create_flat_scene();
        assert!(!vertices.is_empty());
        for v in &vertices {
            assert!(v.position[0].is_finite() && v.position[0].abs() <= 1.0);
            assert!(v.position[1].is_finite() && v.position[1].abs() <= 1.0);
            assert!((0.0..=1.0).contains(&v.position[2]));
        }
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        assert_eq!(indices.len() % 3, 0);
    }

    #[test]
    fn push_box_generates_twenty_four_vertices_and_thirty_six_indices() {
        let mut v = Vec::new();
        let mut i = Vec::new();
        push_box(&mut v, &mut i, [0.0; 3], [1.0; 3], [1.0; 3]);
        assert_eq!(v.len(), 24);
        assert_eq!(i.len(), 36);
    }
}
