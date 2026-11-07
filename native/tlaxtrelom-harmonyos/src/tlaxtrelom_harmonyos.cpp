// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

#include "tlaxtrelom_harmonyos.h"
#include <cstdlib>
#include <cstring>
#include <atomic>

struct HarmonyOsContext {
    TlaxtrelomOhosSurfaceInfo info;
    std::atomic<bool> focused{true};
    std::atomic<bool> active{true};
};

uint32_t tlaxtrelom_harmonyos_abi_version(void) {
    return TLAXTRELOM_HARMONYOS_ABI_VERSION;
}

int32_t tlaxtrelom_harmonyos_init_surface(
    const TlaxtrelomOhosSurfaceInfo* info, void** out_handle)
{
    if (!info || !out_handle) {
        return -1;
    }

    auto* ctx = new (std::nothrow) HarmonyOsContext();
    if (!ctx) {
        return -2;
    }

    ctx->info = *info;
    ctx->focused.store(true);
    ctx->active.store(true);

    *out_handle = static_cast<void*>(ctx);
    return 0;
}

void tlaxtrelom_harmonyos_on_window_focus(void* handle, bool has_focus) {
    if (!handle) return;
    auto* ctx = static_cast<HarmonyOsContext*>(handle);
    ctx->focused.store(has_focus);
}

void tlaxtrelom_harmonyos_on_window_resize(
    void* handle, uint32_t new_width, uint32_t new_height)
{
    if (!handle) return;
    auto* ctx = static_cast<HarmonyOsContext*>(handle);
    ctx->info.width = new_width;
    ctx->info.height = new_height;
}

void tlaxtrelom_harmonyos_destroy_surface(void* handle) {
    if (!handle) return;
    auto* ctx = static_cast<HarmonyOsContext*>(handle);
    delete ctx;
}
