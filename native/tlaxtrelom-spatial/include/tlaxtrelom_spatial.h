/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 *
 * tlaxtrelom_spatial.h — Núcleo de cinemática espacial y transformaciones 6DOF en C/Zig.
 *
 * Proporciona matemáticas de cuaterniones duales (skinning sin colapso de volumen
 * para avatares VR), aceleración BVH para eye-tracking y trazado de rayos.
 */

#ifndef TLAXTRELOM_SPATIAL_H
#define TLAXTRELOM_SPATIAL_H

#include <stdint.h>
#include <stdbool.h>

#define TLAXTRELOM_SPATIAL_ABI_VERSION 1u

#if defined(_WIN32)
#  define TLAXTRELOM_SPATIAL_EXPORT __declspec(dllexport)
#elif defined(__GNUC__) || defined(__clang__)
#  define TLAXTRELOM_SPATIAL_EXPORT __attribute__((visibility("default")))
#else
#  define TLAXTRELOM_SPATIAL_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Cuaternión dual: parte real (rotación) + parte dual (traslación). */
typedef struct TlaxtrelomDualQuat {
    float real[4]; /* [x, y, z, w] */
    float dual[4]; /* [x, y, z, w] */
} TlaxtrelomDualQuat;

/* Caja envolvente alineada a los ejes (AABB) para aceleración espacial. */
typedef struct TlaxtrelomAABB {
    float min_point[3];
    float max_point[3];
} TlaxtrelomAABB;

/* Rayo para pruebas de intersección con el campo de visión / mirada. */
typedef struct TlaxtrelomRay {
    float origin[3];
    float direction[3];
} TlaxtrelomRay;

TLAXTRELOM_SPATIAL_EXPORT uint32_t tlaxtrelom_spatial_abi_version(void);

/* Construye un cuaternión dual a partir de rotación [x,y,z,w] y traslación [x,y,z]. */
TLAXTRELOM_SPATIAL_EXPORT void tlaxtrelom_spatial_dual_quat_from_transform(
    const float rot[4], const float trans[3], TlaxtrelomDualQuat* out);

/* Transforma un punto 3D usando un cuaternión dual. */
TLAXTRELOM_SPATIAL_EXPORT void tlaxtrelom_spatial_dual_quat_transform_point(
    const TlaxtrelomDualQuat* dq, const float in_point[3], float out_point[3]);

/* Intersección de rayo con AABB. Devuelve true si intersecta y la distancia en *t_hit. */
TLAXTRELOM_SPATIAL_EXPORT bool tlaxtrelom_spatial_ray_aabb_intersect(
    const TlaxtrelomRay* ray, const TlaxtrelomAABB* aabb, float* t_hit);

#ifdef __cplusplus
}
#endif

#endif /* TLAXTRELOM_SPATIAL_H */
