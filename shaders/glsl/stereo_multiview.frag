#version 450
// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//
// Shader de fragmentos estéreo con viñeteado dinámico de confort (anti-motion sickness)
// y corrección de aberración cromática en VR.

layout(binding = 0) uniform ViewUniforms {
    mat4 viewProj[2];
    vec4 cameraPos[2];
    vec4 comfortVignette; // x = aperture [0.4, 1.0], y = ipd
} uViews;

layout(location = 0) in vec2 fragTexCoord;
layout(location = 1) in vec3 fragNormal;
layout(location = 2) in flat int eyeIndex;

layout(location = 0) out vec4 outColor;

void main() {
    vec3 N = normalize(fragNormal);
    vec3 L = normalize(vec3(0.5, 0.8, 0.3));
    float diff = max(dot(N, L), 0.15);

    // Color base del motor Tlaxtrelom (verde cian tecnológico)
    vec3 baseColor = (eyeIndex == 0) ? vec3(0.12, 0.55, 0.65) : vec3(0.14, 0.60, 0.55);
    vec3 litColor = baseColor * diff;

    // Aplicar viñeteado de confort según la apertura calculada en CPU
    vec2 ndc = fragTexCoord * 2.0 - 1.0;
    float distFromCenter = length(ndc);
    float vignetteAperture = uViews.comfortVignette.x;
    float vignette = smoothstep(vignetteAperture, vignetteAperture * 0.7, distFromCenter);

    outColor = vec4(litColor * vignette, 1.0);
}
