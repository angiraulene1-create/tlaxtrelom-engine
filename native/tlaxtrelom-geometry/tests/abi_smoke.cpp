// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//
// Smoke test de la ABI C de `tlaxtrelom-geometry`.
//
// Verifica el contrato que Rust asume:
//   - versión de ABI;
//   - rechazo de argumentos nulos;
//   - formato de asset inválido;
//   - carga asíncrona real (pread + zstd) y `ack_upload`;
//   - una página residente no se vuelve a pedir;
//   - los contadores de telemetría son coherentes.
//
// Se compila sin excepciones a través de la ABI: el test usa el mismo header C
// que el runtime, no los tipos internos de C++.

#include "tlaxtrelom_geometry.h"

#include <zstd.h>

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

#include <fcntl.h>
#include <unistd.h>

#define CHECK(condition)                                                        \
    do {                                                                        \
        if (!(condition)) {                                                     \
            std::fprintf(stderr, "CHECK falló en %s:%d: %s\n", __FILE__,       \
                         __LINE__, #condition);                                 \
            return 1;                                                           \
        }                                                                       \
    } while (0)

namespace {

// Espejo del formato cocinado. Cambiar estos layouts exige subir la versión de
// asset y de ABI.
struct AssetHeader {
    uint32_t magic;
    uint16_t version;
    uint16_t flags;
    uint32_t cluster_count;
    uint32_t page_count;
    uint32_t root_count;
    uint32_t reserved;
};
static_assert(sizeof(AssetHeader) == 24, "header de asset estable");

struct Cluster {
    float center[3];
    float radius;
    float lod_error;
    uint32_t page_local;
    uint32_t first_child;
    uint8_t child_count;
    uint8_t _pad[3];
};
static_assert(sizeof(Cluster) == 32, "cluster estable");

struct PageDesc {
    uint64_t offset;
    uint32_t compressed_size;
    uint32_t raw_size;
};
static_assert(sizeof(PageDesc) == 16, "page desc estable");

constexpr uint32_t kAssetMagic = 0x544C5847u; // "TLXG"

struct TempFile {
    std::string path;

    TempFile() {
        char name[] = "/tmp/tlaxtrelom-geo-XXXXXX";
        const int fd = ::mkstemp(name);
        if (fd >= 0) {
            ::close(fd);
            path = name;
        }
    }

    ~TempFile() {
        if (!path.empty()) {
            std::remove(path.c_str());
        }
    }
};

bool write_all(int fd, const void* data, size_t size) {
    const auto* p = static_cast<const uint8_t*>(data);
    size_t done = 0;
    while (done < size) {
        const ssize_t n = ::write(fd, p + done, size - done);
        if (n <= 0) {
            return false;
        }
        done += static_cast<size_t>(n);
    }
    return true;
}

// Escribe un asset de una página con payload comprimido con zstd.
bool write_asset(const std::string& path, const std::vector<uint8_t>& payload) {
    std::vector<uint8_t> compressed(ZSTD_compressBound(payload.size()));
    const size_t compressed_size =
        ZSTD_compress(compressed.data(), compressed.size(), payload.data(),
                      payload.size(), 3);
    if (ZSTD_isError(compressed_size)) {
        return false;
    }
    compressed.resize(compressed_size);

    AssetHeader header{};
    header.magic = kAssetMagic;
    header.version = 1;
    header.cluster_count = 1;
    header.page_count = 1;
    header.root_count = 1;

    Cluster cluster{};
    cluster.center[0] = 0.0f;
    cluster.center[1] = 1.7f;
    cluster.center[2] = 2.0f;
    cluster.radius = 0.5f;
    cluster.lod_error = 0.1f;
    cluster.page_local = 0;
    cluster.first_child = 0;
    cluster.child_count = 0;

    const uint32_t root = 0;

    PageDesc page{};
    const uint64_t data_offset = sizeof(AssetHeader) + sizeof(Cluster) +
                                 sizeof(uint32_t) + sizeof(PageDesc);
    page.offset = data_offset;
    page.compressed_size = static_cast<uint32_t>(compressed.size());
    page.raw_size = static_cast<uint32_t>(payload.size());

    const int fd = ::open(path.c_str(), O_WRONLY | O_TRUNC);
    if (fd < 0) {
        return false;
    }
    const bool ok = write_all(fd, &header, sizeof(header)) &&
                    write_all(fd, &cluster, sizeof(cluster)) &&
                    write_all(fd, &root, sizeof(root)) &&
                    write_all(fd, &page, sizeof(page)) &&
                    write_all(fd, compressed.data(), compressed.size());
    ::close(fd);
    return ok;
}

bool write_bad_asset(const std::string& path) {
    AssetHeader header{};
    header.magic = 0xDEADBEEFu;
    header.version = 1;
    const int fd = ::open(path.c_str(), O_WRONLY | O_TRUNC);
    if (fd < 0) {
        return false;
    }
    const bool ok = write_all(fd, &header, sizeof(header));
    ::close(fd);
    return ok;
}

TlaxtrelomGeoView identity_view() {
    TlaxtrelomGeoView view{};
    view.view_proj[0] = 1.0f;
    view.view_proj[5] = 1.0f;
    view.view_proj[10] = 1.0f;
    view.view_proj[15] = 1.0f;
    view.camera[0] = 0.0f;
    view.camera[1] = 1.7f;
    view.camera[2] = 0.0f;
    view.fov_y_rad = 1.5707963f;
    view.screen_width = 1280;
    view.screen_height = 720;
    view.pixel_error_budget = 1.0f;
    return view;
}

} // namespace

int main() {
    CHECK(tlaxtrelom_geo_abi_version() == 1u);

    const TlaxtrelomGeoConfig bad_cfg{};
    TlaxtrelomGeoStreamer* s = nullptr;
    CHECK(tlaxtrelom_geo_create(nullptr, &s) == TLAXTRELOM_GEO_ERR_INVALID);
    CHECK(tlaxtrelom_geo_create(&bad_cfg, nullptr) == TLAXTRELOM_GEO_ERR_INVALID);

    TempFile asset;
    TempFile malformed;
    CHECK(!asset.path.empty());
    CHECK(!malformed.path.empty());

    const std::vector<uint8_t> payload = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11};
    CHECK(write_asset(asset.path, payload));
    CHECK(write_bad_asset(malformed.path));

    TlaxtrelomGeoConfig cfg{};
    cfg.max_resident_pages = 64;
    cfg.io_threads = 2;
    cfg.vram_budget_bytes = 256ull << 20;
    cfg.cache_dir = "/tmp";
    cfg.max_ready_pages = 8;
    CHECK(tlaxtrelom_geo_create(&cfg, &s) == TLAXTRELOM_GEO_OK);
    CHECK(s != nullptr);

    CHECK(tlaxtrelom_geo_register_asset(s, 1, "/no/existe.tlaxtrelomgeo") ==
          TLAXTRELOM_GEO_ERR_IO);
    CHECK(tlaxtrelom_geo_register_asset(s, 2, malformed.path.c_str()) ==
          TLAXTRELOM_GEO_ERR_FORMAT);
    CHECK(tlaxtrelom_geo_register_asset(s, 3, asset.path.c_str()) ==
          TLAXTRELOM_GEO_OK);

    const TlaxtrelomGeoView view = identity_view();
    CHECK(tlaxtrelom_geo_begin_frame(nullptr, &view, 0) ==
          TLAXTRELOM_GEO_ERR_INVALID);
    CHECK(tlaxtrelom_geo_begin_frame(s, nullptr, 0) ==
          TLAXTRELOM_GEO_ERR_INVALID);
    CHECK(tlaxtrelom_geo_begin_frame(s, &view, 1) == TLAXTRELOM_GEO_OK);

    TlaxtrelomGeoPageRequest requests[8]{};
    const uint32_t requested = tlaxtrelom_geo_collect(s, requests, 8);
    CHECK(requested == 1);
    CHECK(requests[0].asset_id == 3);
    CHECK(requests[0].page_id == ((uint64_t(3) << 32) | 0u));

    TlaxtrelomGeoReadyPage ready[8]{};
    uint32_t ready_count = 0;
    for (int attempt = 0; attempt < 500 && ready_count == 0; ++attempt) {
        ready_count = tlaxtrelom_geo_drain_ready(s, ready, 8);
        if (ready_count == 0) {
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
    }
    CHECK(ready_count == 1);
    CHECK(ready[0].size == payload.size());
    CHECK(ready[0].data != nullptr);
    CHECK(std::memcmp(ready[0].data, payload.data(), payload.size()) == 0);

    const uint64_t page_id = ready[0].page_id;
    CHECK(tlaxtrelom_geo_ack_upload(s, page_id) == TLAXTRELOM_GEO_OK);
    CHECK(tlaxtrelom_geo_ack_upload(s, page_id) == TLAXTRELOM_GEO_ERR_STATE);

    TlaxtrelomGeoStats stats{};
    tlaxtrelom_geo_get_stats(s, &stats);
    CHECK(stats.pages_resident == 1);
    CHECK(stats.page_faults >= 1);
    CHECK(stats.io_bytes > 0);

    // La página ya está residente: el siguiente frame no debe pedirla.
    CHECK(tlaxtrelom_geo_begin_frame(s, &view, 2) == TLAXTRELOM_GEO_OK);
    CHECK(tlaxtrelom_geo_collect(s, requests, 8) == 0);

    tlaxtrelom_geo_destroy(s);
    tlaxtrelom_geo_destroy(nullptr);

    std::printf("abi_smoke: OK (page_id=%llu, bytes=%llu)\n",
                static_cast<unsigned long long>(page_id),
                static_cast<unsigned long long>(stats.io_bytes));
    return 0;
}
