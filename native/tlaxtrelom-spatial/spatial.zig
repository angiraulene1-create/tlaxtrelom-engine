// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

const std = @import("std");

pub const ABI_VERSION: u32 = 1;

pub const DualQuat = extern struct {
    real: [4]f32,
    dual: [4]f32,
};

pub const AABB = extern struct {
    min_point: [3]f32,
    max_point: [3]f32,
};

pub const Ray = extern struct {
    origin: [3]f32,
    direction: [3]f32,
};

export fn tlaxtrelom_spatial_abi_version() callconv(.c) u32 {
    return ABI_VERSION;
}

export fn tlaxtrelom_spatial_dual_quat_from_transform(
    rot: *const [4]f32,
    trans: *const [3]f32,
    out: *DualQuat,
) callconv(.c) void {
    out.real = rot.*;

    // dual = 0.5 * (trans_quat * rot)
    const tx = trans[0];
    const ty = trans[1];
    const tz = trans[2];
    const rx = rot[0];
    const ry = rot[1];
    const rz = rot[2];
    const rw = rot[3];

    out.dual[0] = 0.5 * (tx * rw + ty * rz - tz * ry);
    out.dual[1] = 0.5 * (-tx * rz + ty * rw + tz * rx);
    out.dual[2] = 0.5 * (tx * ry - ty * rx + tz * rw);
    out.dual[3] = 0.5 * (-tx * rx - ty * ry - tz * rz);
}

export fn tlaxtrelom_spatial_dual_quat_transform_point(
    dq: *const DualQuat,
    in_point: *const [3]f32,
    out_point: *[3]f32,
) callconv(.c) void {
    const rx = dq.real[0];
    const ry = dq.real[1];
    const rz = dq.real[2];
    const rw = dq.real[3];

    // Traslación = 2 * (dual * conjugate(real))
    const t0 = 2.0 * (-dq.dual[3] * rx + dq.dual[0] * rw - dq.dual[1] * rz + dq.dual[2] * ry);
    const t1 = 2.0 * (-dq.dual[3] * ry + dq.dual[0] * rz + dq.dual[1] * rw - dq.dual[2] * rx);
    const t2 = 2.0 * (-dq.dual[3] * rz - dq.dual[0] * ry + dq.dual[1] * rx + dq.dual[2] * rw);

    // Rotar in_point con q_real
    const px = in_point[0];
    const py = in_point[1];
    const pz = in_point[2];

    const cx = ry * pz - rz * py;
    const cy = rz * px - rx * pz;
    const cz = rx * py - ry * px;

    const ccx = ry * cz - rz * cy;
    const ccy = rz * cx - rx * cz;
    const ccz = rx * cy - ry * cx;

    out_point[0] = px + 2.0 * (rw * cx + ccx) + t0;
    out_point[1] = py + 2.0 * (rw * cy + ccy) + t1;
    out_point[2] = pz + 2.0 * (rw * cz + ccz) + t2;
}

export fn tlaxtrelom_spatial_ray_aabb_intersect(
    ray: *const Ray,
    aabb: *const AABB,
    t_hit: *f32,
) callconv(.c) bool {
    var tmin: f32 = 0.0;
    var tmax: f32 = std.math.floatMax(f32);

    inline for (0..3) |i| {
        const inv_d = 1.0 / ray.direction[i];
        var t0 = (aabb.min_point[i] - ray.origin[i]) * inv_d;
        var t1 = (aabb.max_point[i] - ray.origin[i]) * inv_d;
        if (inv_d < 0.0) {
            const tmp = t0;
            t0 = t1;
            t1 = tmp;
        }
        tmin = @max(tmin, t0);
        tmax = @min(tmax, t1);
        if (tmax < tmin) return false;
    }

    t_hit.* = tmin;
    return true;
}
