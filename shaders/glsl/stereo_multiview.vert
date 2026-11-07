#version 450
// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//
// Shader de vértices estéreo con multiview/instancing para Tlaxtrelom Engine.

layout(location = 0) in vec3 inPosition;
layout(location = 1) in vec2 inTexCoord;
layout(location = 2) in vec3 inNormal;

layout(binding = 0) uniform ViewUniforms {
    mat4 viewProj[2]; // [0] = Left Eye, [1] = Right Eye
    vec4 cameraPos[2];
    vec4 comfortVignette; // x = aperture [0.4, 1.0], y = ipd
} uViews;

layout(location = 0) out vec2 fragTexCoord;
layout(location = 1) out vec3 fragNormal;
layout(location = 2) out flat int eyeIndex;

void main() {
    // Si gl_ViewIndex no está disponible, se usa gl_InstanceIndex & 1
    int eye = int(gl_InstanceIndex & 1);
    eyeIndex = eye;
    fragTexCoord = inTexCoord;
    fragNormal = inNormal;

    gl_Position = uViews.viewProj[eye] * vec4(inPosition, 1.0);
}
