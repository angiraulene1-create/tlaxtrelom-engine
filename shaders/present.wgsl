// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//
// Pase de presentación: toma el HDR lineal del frame offscreen, lo expone,
// lo tone-mapea y lo escribe en el swapchain. Es el único punto del pipeline
// que comprime rango dinámico; los pases de escena deben entregar lineal.

struct Present {
    resolution: vec2<f32>,
    time: f32,
    exposure: f32,
}

@group(0) @binding(0) var hdr_texture: texture_2d<f32>;
@group(0) @binding(1) var hdr_sampler: sampler;
@group(0) @binding(2) var<uniform> present: Present;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(p[i], 0.0, 1.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Diente de león (blue-noise aproximado): con destino de 8 bits por canal,
// un gradiente suave bandea si se cuantiza a secas.
fn dither(frag: vec2<f32>) -> f32 {
    let q = fract(frag * vec2<f32>(0.06711056, 0.00583715));
    return fract(52.9829189 * q.x * q.y) - 0.5;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let res = max(present.resolution, vec2<f32>(1.0, 1.0));
    let uv = frag.xy / res;
    let hdr = max(textureSample(hdr_texture, hdr_sampler, uv).rgb, vec3<f32>(0.0));
    let mapped = aces(hdr * max(present.exposure, 0.0));
    // Cuantizar en lineal y dejar que el formato sRGB del swapchain haga la
    // codificación: codificar dos veces oscurece toda la imagen.
    return vec4<f32>(mapped + dither(frag.xy) / 255.0, 1.0);
}
