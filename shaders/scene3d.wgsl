// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//
// Pipeline de renderizado 3D para mallas estáticas y geometría virtual transmitida.

struct SceneUniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>, // xyz = posición de cámara, w = tiempo
    light_dir: vec4<f32>,  // xyz = dirección de luz normalizada, w = ojo (0:izq, 1:der, 2:mono)
};

@group(0) @binding(0) var<uniform> scene: SceneUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.world_pos = model.position;
    out.normal = normalize(model.normal);
    out.color = model.color;
    out.clip_pos = scene.view_proj * vec4<f32>(model.position, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light = normalize(scene.light_dir.xyz);
    let n = normalize(in.normal);
    let view_dir = normalize(scene.camera_pos.xyz - in.world_pos);

    // Iluminación ambiental y difusa (Lambert)
    let ndotl = max(dot(n, light), 0.0);
    let ambient = 0.22;
    let diffuse = ndotl * 0.70;

    // Especular sutil (Blinn-Phong)
    let half_dir = normalize(light + view_dir);
    let spec = pow(max(dot(n, half_dir), 0.0), 32.0) * 0.35;

    // Sombreado de color
    let lit_color = in.color * (ambient + diffuse) + vec3<f32>(spec);

    // Niebla de distancia exponencial suave para acentuar profundidad espacial
    let dist = length(scene.camera_pos.xyz - in.world_pos);
    let fog = clamp((dist - 8.0) / 45.0, 0.0, 0.80);
    let fog_color = vec3<f32>(0.012, 0.016, 0.028);

    let final_color = mix(lit_color, fog_color, fog);
    return vec4<f32>(final_color, 1.0);
}
