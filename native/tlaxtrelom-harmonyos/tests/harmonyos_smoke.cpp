// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

#include "tlaxtrelom_harmonyos.h"
#include <cassert>
#include <cstdio>
#include <cstring>

int main(void) {
    uint32_t abi = tlaxtrelom_harmonyos_abi_version();
    assert(abi == 1);

    TlaxtrelomOhosSurfaceInfo info{};
    info.window_id = 998877;
    info.width = 2560;
    info.height = 1440;
    info.refresh_rate_hz = 120.0f;
    info.is_hdr_capable = true;
    std::strncpy(info.display_name, "HUAWEI MateView PC Display", sizeof(info.display_name) - 1);

    void* handle = nullptr;
    int32_t rc = tlaxtrelom_harmonyos_init_surface(&info, &handle);
    assert(rc == 0);
    assert(handle != nullptr);

    tlaxtrelom_harmonyos_on_window_resize(handle, 3840, 2160);
    tlaxtrelom_harmonyos_on_window_focus(handle, false);
    tlaxtrelom_harmonyos_destroy_surface(handle);

    std::printf("harmonyos_smoke: OK (abi=%u, initial_res=%ux%u)\n", abi, info.width, info.height);
    return 0;
}
