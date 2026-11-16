// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//
// Pipeline de renderizado para mallas estáticas, geometría virtual transmitida
// y politopos N-D proyectados. El mismo shader sirve a 2D, 2.5D, 3D, 4D, 4.5D
// y 5D: la diferencia está en la matriz view-projection y en `params`.
//
// Entrega COLOR LINEAL de rango alto al target HDR del frame; el tone-mapping
// vive exclusivamente en `present.wgsl`.

struct SceneUniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>, // xyz = posición de cámara, w = tiempo
    light_dir: vec4<f32>,  // xyz = dirección de luz, w = ojo (0:izq, 1:der, 2:mono)
    // x = tiempo, y = código de dimensión, z = blend del modo fraccionario
    // (3.5d orto↔persp, 4.5d 3D↔4D), w = unlit (1 = 2D)
    params: vec4<f32>,
    // x = inicio de la niebla, y = rampa, z = densidad máxima (metros)
    fog: vec4<f32>,
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
    let albedo = max(in.color, vec3<f32>(0.0));

    // Modo 2D: la escena ya viene compuesta en NDC; se presenta plana.
    if (scene.params.w > 0.5) {
        return vec4<f32>(albedo, 1.0);
    }

    let light = normalize(scene.light_dir.xyz);
    let n = normalize(in.normal);
    let view_dir = normalize(scene.camera_pos.xyz - in.world_pos);

    // Key light (Lambert) + luz de relleno fría desde el lado opuesto.
    let ndotl = max(dot(n, light), 0.0);
    let ambient = 0.14 + 0.10 * max(n.y, 0.0);
    let diffuse = ndotl * 0.78;
    let fill_dir = normalize(vec3<f32>(-light.x, 0.35, -light.z));
    let fill = max(dot(n, fill_dir), 0.0) * 0.22;

    // Especular Blinn-Phong con exponente dependiente del blend dimensional.
    let half_dir = normalize(light + view_dir);
    let spec_power = 24.0 + 40.0 * clamp(scene.params.z, 0.0, 1.0);
    let spec = pow(max(dot(n, half_dir), 0.0), spec_power) * 0.30;

    // Rim light: separa siluetas del fondo, especialmente útil en VR.
    let rim = pow(1.0 - max(dot(n, view_dir), 0.0), 3.0) * 0.18;

    var lit = albedo * (ambient + diffuse + fill) + vec3<f32>(spec + rim);

    // Niebla de distancia exponencial suave para acentuar profundidad espacial.
    // Su color coincide con el clear del frame, así que en present.wgsl se
    // tone-mapea igual que el fondo: la distancia no cambia de tinte.
    let dist = length(scene.camera_pos.xyz - in.world_pos);
    let fog = clamp((dist - scene.fog.x) / max(scene.fog.y, 1.0), 0.0, scene.fog.z);
    let fog_color = vec3<f32>(0.004, 0.006, 0.012);
    lit = mix(lit, fog_color, fog);

    return vec4<f32>(lit, 1.0);
}
