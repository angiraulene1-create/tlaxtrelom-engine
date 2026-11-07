/* SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
 * Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
 * CONFIDENTIAL — internal use only.
 */

#include "tlaxtrelom_spatial.h"
#include <assert.h>
#include <stdio.h>
#include <math.h>

int main(void) {
    uint32_t abi = tlaxtrelom_spatial_abi_version();
    assert(abi == 1);

    /* Test 1: Transformación identidad */
    float rot_ident[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    float trans_offset[3] = {10.0f, 20.0f, 30.0f};
    TlaxtrelomDualQuat dq;
    tlaxtrelom_spatial_dual_quat_from_transform(rot_ident, trans_offset, &dq);

    float p_in[3] = {1.0f, 2.0f, 3.0f};
    float p_out[3] = {0.0f, 0.0f, 0.0f};
    tlaxtrelom_spatial_dual_quat_transform_point(&dq, p_in, p_out);

    assert(fabsf(p_out[0] - 11.0f) < 1e-4f);
    assert(fabsf(p_out[1] - 22.0f) < 1e-4f);
    assert(fabsf(p_out[2] - 33.0f) < 1e-4f);

    /* Test 2: Intersección Ray-AABB */
    TlaxtrelomRay ray = {
        .origin = {0.0f, 0.0f, -5.0f},
        .direction = {0.0f, 0.0f, 1.0f}
    };
    TlaxtrelomAABB aabb = {
        .min_point = {-1.0f, -1.0f, 2.0f},
        .max_point = {1.0f, 1.0f, 4.0f}
    };
    float t_hit = 0.0f;
    bool hit = tlaxtrelom_spatial_ray_aabb_intersect(&ray, &aabb, &t_hit);
    assert(hit);
    assert(fabsf(t_hit - 7.0f) < 1e-4f);

    /* Rayo que no golpea */
    TlaxtrelomRay miss_ray = {
        .origin = {10.0f, 10.0f, -5.0f},
        .direction = {0.0f, 0.0f, 1.0f}
    };
    bool miss = tlaxtrelom_spatial_ray_aabb_intersect(&miss_ray, &aabb, &t_hit);
    assert(!miss);

    printf("spatial_smoke: OK (abi=%u, t_hit=%.2f)\n", abi, t_hit);
    return 0;
}
