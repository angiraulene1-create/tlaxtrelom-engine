// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//
// Shader de presentación estéreo/preview del runtime.
// Un solo triángulo fullscreen; el ojo se selecciona por uniform buffer.
// En el motor real este pase se sustituye por el frame graph con
// visibility buffer + lighting; aquí valida el camino estéreo y el pacing.

struct Globals {
    resolution: vec2<f32>,
    time: f32,
    eye: f32,          // 0 = izquierdo, 1 = derecho, 2 = mono
    ipd: f32,
    fov_y: f32,
    is_stereo: f32,
    dimension: f32,    // 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0
};

@group(0) @binding(0) var<uniform> g: Globals;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(p[i], 0.0, 1.0);
}

fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 345.45));
    let r = fract(q.x * q.y) * 43758.5453;
    return fract(r);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let res = max(g.resolution, vec2<f32>(1.0, 1.0));
    let uv = clamp(frag.xy / res, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    var centered = uv * 2.0 - vec2<f32>(1.0, 1.0);
    let aspect = res.x / res.y;
    let stereo = select(0.0, 1.0, g.is_stereo > 0.5);
    let eye_sign = select(0.0, select(-1.0, 1.0, g.eye > 0.5), g.is_stereo > 0.5);

    // Modo 2D: fondo plano de "papel" con rejilla fina. La escena 2D real se
    // dibuja encima con el pipeline de mallas en coordenadas NDC.
    if (g.dimension < 2.5) {
        var flat = vec3<f32>(0.018, 0.028, 0.052);
        let grid = max(
            smoothstep(0.985, 1.0, abs(sin(uv.x * 64.0))),
            smoothstep(0.985, 1.0, abs(sin(uv.y * 36.0))),
        );
        flat = flat + vec3<f32>(0.035, 0.090, 0.130) * grid;
        let glow = exp(-length(centered * vec2<f32>(aspect, 1.0)) * 2.2);
        flat = flat + vec3<f32>(0.020, 0.070, 0.090) * glow;
        let vig2 = smoothstep(1.40, 0.30, length(centered));
        return vec4<f32>(flat * (0.70 + 0.30 * vig2), 1.0);
    }

    // Paralaje horizontal dependiente de la IPD y de la distancia al centro.
    // No sustituye a la proyección por ojo; evita el "doble plano" plano del
    // preview y hace visible cualquier error de separación.
    let parallax = eye_sign * (g.ipd * 2.5) * (1.0 - centered.y * centered.y);
    centered.x = centered.x + parallax;

    var col = vec3<f32>(0.018, 0.024, 0.045);

    // Horizonte y suelo con rejilla.
    let horizon = smoothstep(0.08, 0.0, abs(centered.y + 0.02));
    col = col + vec3<f32>(0.06, 0.20, 0.24) * horizon * 0.55;

    let ground = smoothstep(0.0, -0.35, centered.y);
    let grid_x = smoothstep(0.96, 1.0, abs(sin(centered.x * 18.0)));
    let grid_z = smoothstep(0.96, 1.0, abs(sin(centered.y * 42.0 + g.time * 0.4)));
    col = col + vec3<f32>(0.03, 0.16, 0.14) * ground * (grid_x + grid_z * 0.6);

    // Onda de energía del motor.
    let wave = 0.5 + 0.5 * sin(centered.x * 5.0 + centered.y * 3.0 + g.time * 1.3);
    col = col + vec3<f32>(0.08, 0.32, 0.26) * wave * 0.28 * (1.0 - ground * 0.5);

    // Tinte hiperdimensional para 4D/4.5D/5D: distingue visualmente el modo
    // sin afirmar que exista física de dimensiones extra.
    let hyper = smoothstep(3.5, 4.5, g.dimension);
    col = col + vec3<f32>(0.11, 0.02, 0.20) * hyper * (0.35 + 0.65 * wave);

    // Retícula central + anillo de seguimiento ocular.
    let d = length((centered - vec2<f32>(0.0, 0.02)) * vec2<f32>(aspect, 1.0));
    let ring = smoothstep(0.014, 0.0, abs(d - 0.20));
    col = col + vec3<f32>(0.75, 0.95, 1.0) * ring * 0.9;
    let dotv = smoothstep(0.010, 0.0, d);
    col = col + vec3<f32>(1.0, 0.36, 0.10) * dotv;

    // Marca de ojo en preview: pequeña barra superior izquierda/derecha.
    let eye_tag = select(0.0, 1.0, g.eye > 0.5);
    let tag_region = step(0.03, uv.y) * step(uv.y, 0.07) *
                     step(0.02, uv.x) * step(uv.x, 0.08);
    col = col + vec3<f32>(0.20 + 0.75 * eye_tag, 0.55, 0.45) * tag_region * 0.6;

    // Ruido escaso anti-banding + viñeta.
    let n = hash21(frag.xy + vec2<f32>(g.time * 7.0, 0.0)) - 0.5;
    col = col + vec3<f32>(n * 0.012);
    let vig = smoothstep(1.35, 0.25, length(centered));
    col = col * (0.72 + 0.28 * vig);

    return vec4<f32>(col, 1.0);
}
