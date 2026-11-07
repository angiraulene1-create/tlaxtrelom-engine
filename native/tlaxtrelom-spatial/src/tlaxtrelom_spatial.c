/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 */

#include "tlaxtrelom_spatial.h"
#include <math.h>

uint32_t tlaxtrelom_spatial_abi_version(void) {
    return TLAXTRELOM_SPATIAL_ABI_VERSION;
}

void tlaxtrelom_spatial_dual_quat_from_transform(
    const float rot[4], const float trans[3], TlaxtrelomDualQuat* out)
{
    if (!rot || !trans || !out) return;

    out->real[0] = rot[0];
    out->real[1] = rot[1];
    out->real[2] = rot[2];
    out->real[3] = rot[3];

    float tx = trans[0], ty = trans[1], tz = trans[2];
    float rx = rot[0], ry = rot[1], rz = rot[2], rw = rot[3];

    out->dual[0] = 0.5f * (tx * rw + ty * rz - tz * ry);
    out->dual[1] = 0.5f * (-tx * rz + ty * rw + tz * rx);
    out->dual[2] = 0.5f * (tx * ry - ty * rx + tz * rw);
    out->dual[3] = 0.5f * (-tx * rx - ty * ry - tz * rz);
}

void tlaxtrelom_spatial_dual_quat_transform_point(
    const TlaxtrelomDualQuat* dq, const float in_point[3], float out_point[3])
{
    if (!dq || !in_point || !out_point) return;

    float rx = dq->real[0], ry = dq->real[1], rz = dq->real[2], rw = dq->real[3];

    float t0 = 2.0f * (-dq->dual[3] * rx + dq->dual[0] * rw - dq->dual[1] * rz + dq->dual[2] * ry);
    float t1 = 2.0f * (-dq->dual[3] * ry + dq->dual[0] * rz + dq->dual[1] * rw - dq->dual[2] * rx);
    float t2 = 2.0f * (-dq->dual[3] * rz - dq->dual[0] * ry + dq->dual[1] * rx + dq->dual[2] * rw);

    float px = in_point[0], py = in_point[1], pz = in_point[2];

    float cx = ry * pz - rz * py;
    float cy = rz * px - rx * pz;
    float cz = rx * py - ry * px;

    float ccx = ry * cz - rz * cy;
    float ccy = rz * cx - rx * cz;
    float ccz = rx * cy - ry * cx;

    out_point[0] = px + 2.0f * (rw * cx + ccx) + t0;
    out_point[1] = py + 2.0f * (rw * cy + ccy) + t1;
    out_point[2] = pz + 2.0f * (rw * cz + ccz) + t2;
}

bool tlaxtrelom_spatial_ray_aabb_intersect(
    const TlaxtrelomRay* ray, const TlaxtrelomAABB* aabb, float* t_hit)
{
    if (!ray || !aabb || !t_hit) return false;

    float tmin = 0.0f;
    float tmax = 1e30f;

    for (int i = 0; i < 3; i++) {
        float dir = ray->direction[i];
        if (fabsf(dir) < 1e-8f) {
            if (ray->origin[i] < aabb->min_point[i] || ray->origin[i] > aabb->max_point[i]) {
                return false;
            }
            continue;
        }

        float inv_d = 1.0f / dir;
        float t0 = (aabb->min_point[i] - ray->origin[i]) * inv_d;
        float t1 = (aabb->max_point[i] - ray->origin[i]) * inv_d;
        if (inv_d < 0.0f) {
            float tmp = t0;
            t0 = t1;
            t1 = tmp;
        }

        if (t0 > tmin) tmin = t0;
        if (t1 < tmax) tmax = t1;
        if (tmax < tmin) return false;
    }

    *t_hit = tmin;
    return true;
}
