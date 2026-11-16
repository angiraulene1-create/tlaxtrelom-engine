// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//
// Smoke test de la ABI C de `tlaxtrelom-geometry`.
//
// Verifica el contrato que Rust asume:
//   - versión de ABI;
//   - rechazo de argumentos nulos;
//   - formato de asset inválido;
//   - carga asíncrona real (pread + zstd) y `ack_upload`;
//   - una página residente no se vuelve a pedir;
//   - una página en vuelo no se duplica ni suma page faults;
//   - las colisiones de slot entre assets se resuelven por desalojo;
//   - el frustum poda los clusters fuera de vista;
//   - la distancia del LOD conserva separaciones de metros sobre un anclaje de
//     1e11 m (aritmética `double`);
//   - un asset del formato anterior se rechaza;
//   - los contadores de telemetría son coherentes.
//
// Se compila sin excepciones a través de la ABI: el test usa el mismo header C
// que el runtime, no los tipos internos de C++.

#include "tlaxtrelom_geometry.h"

#include <zstd.h>

#include <chrono>
#include <cmath>
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
    double origin[3];
};
static_assert(sizeof(AssetHeader) == 48, "header de asset estable");

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
constexpr uint16_t kSmokeAssetVersion = 2;

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

// Espejo del `Cluster` cocinado, pero con el centro en `double` para poder
// colocar clusters a distancias que el `float` no distinguiría.
struct SmokeCluster {
    double center[3];
    float radius;
    float lod_error;
    uint32_t page_local;
    uint32_t first_child;
    uint8_t child_count;
};

// Escribe un asset v2: un DAG de `clusters` (todos raíz, salvo que se indiquen
// hijos) y una página por payload, comprimida con zstd.
bool write_asset_v2(const std::string& path, const double origin[3],
                    const std::vector<SmokeCluster>& clusters,
                    const std::vector<std::vector<uint8_t>>& pages) {
    AssetHeader header{};
    header.magic = kAssetMagic;
    header.version = kSmokeAssetVersion;
    header.cluster_count = static_cast<uint32_t>(clusters.size());
    header.page_count = static_cast<uint32_t>(pages.size());
    header.root_count = static_cast<uint32_t>(clusters.size());
    for (int i = 0; i < 3; ++i) header.origin[i] = origin[i];

    std::vector<Cluster> on_disk(clusters.size());
    for (size_t i = 0; i < clusters.size(); ++i) {
        const SmokeCluster& c = clusters[i];
        for (int axis = 0; axis < 3; ++axis) {
            on_disk[i].center[axis] = static_cast<float>(c.center[axis]);
        }
        on_disk[i].radius = c.radius;
        on_disk[i].lod_error = c.lod_error;
        on_disk[i].page_local = c.page_local;
        on_disk[i].first_child = c.first_child;
        on_disk[i].child_count = c.child_count;
    }
    std::vector<uint32_t> roots(clusters.size());
    for (size_t i = 0; i < roots.size(); ++i) roots[i] = static_cast<uint32_t>(i);

    const uint64_t table_size = sizeof(AssetHeader) +
                                on_disk.size() * sizeof(Cluster) +
                                roots.size() * sizeof(uint32_t) +
                                pages.size() * sizeof(PageDesc);
    std::vector<PageDesc> descs(pages.size());
    std::vector<std::vector<uint8_t>> blobs(pages.size());
    uint64_t offset = table_size;
    for (size_t i = 0; i < pages.size(); ++i) {
        std::vector<uint8_t> compressed(ZSTD_compressBound(pages[i].size()));
        const size_t n = ZSTD_compress(compressed.data(), compressed.size(),
                                       pages[i].data(), pages[i].size(), 3);
        if (ZSTD_isError(n)) return false;
        compressed.resize(n);
        descs[i] = PageDesc{offset, static_cast<uint32_t>(compressed.size()),
                            static_cast<uint32_t>(pages[i].size())};
        offset += compressed.size();
        blobs[i] = std::move(compressed);
    }

    const int fd = ::open(path.c_str(), O_WRONLY | O_TRUNC);
    if (fd < 0) return false;
    bool ok = write_all(fd, &header, sizeof(header)) &&
              write_all(fd, on_disk.data(), on_disk.size() * sizeof(Cluster)) &&
              write_all(fd, roots.data(), roots.size() * sizeof(uint32_t)) &&
              write_all(fd, descs.data(), descs.size() * sizeof(PageDesc));
    for (const auto& blob : blobs) {
        ok = ok && write_all(fd, blob.data(), blob.size());
    }
    ::close(fd);
    return ok;
}

// Escribe un asset de una página con payload comprimido con zstd.
bool write_asset(const std::string& path, const std::vector<uint8_t>& payload) {
    const double origin[3] = {0.0, 0.0, 0.0};
    const std::vector<SmokeCluster> clusters = {
        SmokeCluster{{0.0f, 1.7f, 2.0f}, 0.5f, 0.1f, 0u, 0u, 0u}};
    return write_asset_v2(path, origin, clusters, {payload});
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

// Producto column-major (out = A·B), el mismo orden que usa la ABI.
void mat_mul(const float* a, const float* b, float* out) {
    for (int col = 0; col < 4; ++col) {
        for (int row = 0; row < 4; ++row) {
            float sum = 0.0f;
            for (int k = 0; k < 4; ++k) {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
}

// Vista de cámara real: perspectiva de 90° con profundidad 0..1 (la convención
// de `xr::perspective`, left-handed, +Z adelante) y la vista traducida por el
// offset local de cámara. El streamer deriva de aquí su frustum, así que una
// `view_proj` identidad ya no vale para ejercitar el recorrido.
TlaxtrelomGeoView camera_view(const double anchor[3], const float camera[3]) {
    const float near = 0.05f;
    const float far = 10000.0f;
    const float aspect = 1280.0f / 720.0f;
    const float f = 1.0f / std::tan(1.5707963f * 0.5f);
    const float d = far - near;
    const float proj[16] = {
        f / aspect, 0.0f, 0.0f, 0.0f, 0.0f, f, 0.0f, 0.0f,
        0.0f, 0.0f, far / d, 1.0f, 0.0f, 0.0f, -near * far / d, 0.0f,
    };
    const float view[16] = {
        1.0f, 0.0f, 0.0f, 0.0f, 0.0f, 1.0f, 0.0f, 0.0f,
        0.0f, 0.0f, 1.0f, 0.0f, -camera[0], -camera[1], -camera[2], 1.0f,
    };

    TlaxtrelomGeoView out{};
    mat_mul(proj, view, out.view_proj);
    for (int i = 0; i < 3; ++i) {
        out.camera[i] = camera[i];
        out.origin[i] = anchor[i];
    }
    out.fov_y_rad = 1.5707963f;
    out.screen_width = 1280;
    out.screen_height = 720;
    out.pixel_error_budget = 1.0f;
    return out;
}

TlaxtrelomGeoView default_view() {
    const double anchor[3] = {0.0, 0.0, 0.0};
    const float camera[3] = {0.0f, 1.7f, 0.0f};
    return camera_view(anchor, camera);
}

TlaxtrelomGeoStreamer* create_streamer(uint32_t slots) {
    TlaxtrelomGeoConfig cfg{};
    cfg.max_resident_pages = slots;
    cfg.io_threads = 1;
    cfg.vram_budget_bytes = 1ull << 20;
    cfg.cache_dir = "/tmp";
    cfg.max_ready_pages = 8;
    TlaxtrelomGeoStreamer* s = nullptr;
    if (tlaxtrelom_geo_create(&cfg, &s) != TLAXTRELOM_GEO_OK) return nullptr;
    return s;
}

// El frustum se deriva de `view_proj`: un cluster a espalda de la cámara no
// debe pedir páginas, y uno delante sí. En un mundo grande esto es la
// diferencia entre leer unas pocas páginas y leer el asset entero.
int frustum_culls_offscreen_clusters(const std::string& path) {
    const double anchor[3] = {0.0, 0.0, 0.0};
    const float camera[3] = {0.0f, 1.7f, 0.0f};
    const std::vector<SmokeCluster> clusters = {
        SmokeCluster{{0.0, 1.7, 5.0}, 0.5f, 1.0f, 0u, 0u, 0u},   // delante
        SmokeCluster{{0.0, 1.7, -50.0}, 0.5f, 1.0f, 1u, 0u, 0u}, // detrás
    };
    CHECK(write_asset_v2(path, anchor, clusters,
                         {std::vector<uint8_t>(16, 7), std::vector<uint8_t>(16, 9)}));

    TlaxtrelomGeoStreamer* s = create_streamer(8);
    CHECK(s != nullptr);
    CHECK(tlaxtrelom_geo_register_asset(s, 5, path.c_str()) == TLAXTRELOM_GEO_OK);
    const TlaxtrelomGeoView view = camera_view(anchor, camera);
    CHECK(tlaxtrelom_geo_begin_frame(s, &view, 1) == TLAXTRELOM_GEO_OK);

    TlaxtrelomGeoPageRequest requests[8]{};
    const uint32_t n = tlaxtrelom_geo_collect(s, requests, 8);
    CHECK(n == 1);
    CHECK(requests[0].page_id == ((uint64_t(5) << 32) | 0u));

    TlaxtrelomGeoStats stats{};
    tlaxtrelom_geo_get_stats(s, &stats);
    CHECK(stats.clusters_culled == 1);

    tlaxtrelom_geo_destroy(s);
    return 0;
}

// La distancia del LOD se calcula en `double` contra el origen del sector. Con
// `float` absoluto, un anclaje a 1e11 m (≈6.7 UA) tiene un ULP de ~8 km: dos
// clusters separados 200 m dan exactamente la misma distancia y el mismo
// priority, y la jerarquía deja de refinar donde hace falta.
int lod_distance_stays_double_at_galactic_anchor(const std::string& path) {
    const double anchor[3] = {1e11, 0.0, 0.0};
    const float camera[3] = {0.0f, 1.7f, 0.0f};
    // Origen del asset en el sector lejano; los centers son offsets pequeños.
    const std::vector<SmokeCluster> clusters = {
        SmokeCluster{{0.0, 1.7, 900.0}, 1.0f, 1000.0f, 0u, 0u, 0u},
        SmokeCluster{{0.0, 1.7, 1100.0}, 1.0f, 1000.0f, 1u, 0u, 0u},
    };
    CHECK(write_asset_v2(path, anchor, clusters,
                         {std::vector<uint8_t>(16, 7), std::vector<uint8_t>(16, 9)}));

    TlaxtrelomGeoStreamer* s = create_streamer(8);
    CHECK(s != nullptr);
    CHECK(tlaxtrelom_geo_register_asset(s, 7, path.c_str()) == TLAXTRELOM_GEO_OK);
    const TlaxtrelomGeoView view = camera_view(anchor, camera);
    CHECK(tlaxtrelom_geo_begin_frame(s, &view, 1) == TLAXTRELOM_GEO_OK);

    TlaxtrelomGeoPageRequest requests[8]{};
    const uint32_t n = tlaxtrelom_geo_collect(s, requests, 8);
    // Que se pidan ambos ya prueba que `view.origin` se resta: ignorarlo dejaría
    // los clusters a 1e11 m por detrás de la cámara, fuera del frustum.
    CHECK(n == 2);
    CHECK(requests[0].priority != requests[1].priority);
    // Orden descendente: el cluster de 900 m es el urgente.
    CHECK(requests[0].page_id == ((uint64_t(7) << 32) | 0u));
    CHECK(requests[0].priority > requests[1].priority);

    tlaxtrelom_geo_destroy(s);
    return 0;
}

// Un asset del formato anterior debe rechazarse, no leerse con los campos
// desalineados: el `origin` caería dentro de la tabla de clusters.
int rejects_the_previous_asset_version(const std::string& path) {
    AssetHeader header{};
    header.magic = kAssetMagic;
    header.version = 1;
    header.cluster_count = 1;
    header.page_count = 1;
    header.root_count = 1;
    const int fd = ::open(path.c_str(), O_WRONLY | O_TRUNC | O_CREAT, 0644);
    if (fd < 0) return 1;
    const bool written = write_all(fd, &header, sizeof(header));
    ::close(fd);
    CHECK(written);

    TlaxtrelomGeoStreamer* s = create_streamer(8);
    CHECK(s != nullptr);
    CHECK(tlaxtrelom_geo_register_asset(s, 8, path.c_str()) ==
          TLAXTRELOM_GEO_ERR_FORMAT);
    tlaxtrelom_geo_destroy(s);
    return 0;
}

int slot_collision_loads_both_pages(const std::string& asset_path) {
    // Con un solo slot, las páginas de dos assets distintos colisionan. Antes
    // del arreglo, la segunda página nunca podía cargarse porque el slot se
    // quedaba residente con el `tag` de la primera.
    TlaxtrelomGeoConfig cfg{};
    cfg.max_resident_pages = 1;
    cfg.io_threads = 1;
    cfg.vram_budget_bytes = 1ull << 20;
    cfg.cache_dir = "/tmp";
    cfg.max_ready_pages = 4;

    TlaxtrelomGeoStreamer* s = nullptr;
    CHECK(tlaxtrelom_geo_create(&cfg, &s) == TLAXTRELOM_GEO_OK);
    CHECK(s != nullptr);
    CHECK(tlaxtrelom_geo_register_asset(s, 10, asset_path.c_str()) ==
          TLAXTRELOM_GEO_OK);
    CHECK(tlaxtrelom_geo_register_asset(s, 11, asset_path.c_str()) ==
          TLAXTRELOM_GEO_OK);

    const TlaxtrelomGeoView view = default_view();
    const uint64_t page_a = uint64_t(10) << 32;
    const uint64_t page_b = uint64_t(11) << 32;
    bool got_a = false;
    bool got_b = false;

    for (uint64_t frame = 1; frame <= 200 && !(got_a && got_b); ++frame) {
        CHECK(tlaxtrelom_geo_begin_frame(s, &view, frame) ==
              TLAXTRELOM_GEO_OK);

        TlaxtrelomGeoPageRequest requests[8]{};
        (void)tlaxtrelom_geo_collect(s, requests, 8);

        TlaxtrelomGeoReadyPage ready[4]{};
        const uint32_t count = tlaxtrelom_geo_drain_ready(s, ready, 4);
        for (uint32_t i = 0; i < count; ++i) {
            got_a = got_a || ready[i].page_id == page_a;
            got_b = got_b || ready[i].page_id == page_b;
            CHECK(tlaxtrelom_geo_ack_upload(s, ready[i].page_id) ==
                  TLAXTRELOM_GEO_OK);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }

    CHECK(got_a);
    CHECK(got_b);

    TlaxtrelomGeoStats stats{};
    tlaxtrelom_geo_get_stats(s, &stats);
    CHECK(stats.pages_resident <= 1); // presupuesto de un solo slot

    tlaxtrelom_geo_destroy(s);
    return 0;
}

} // namespace

int main() {
    CHECK(tlaxtrelom_geo_abi_version() == 2u);

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

    const TlaxtrelomGeoView view = default_view();
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

    // Mientras la página está en vuelo (loading/ready) no se vuelve a pedir ni
    // se contabiliza otro page fault.
    TlaxtrelomGeoPageRequest in_flight_requests[8]{};
    CHECK(tlaxtrelom_geo_collect(s, in_flight_requests, 8) == 0);

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
    CHECK(stats.page_faults == 1);
    CHECK(stats.io_bytes > 0);

    // La página ya está residente: el siguiente frame no debe pedirla.
    CHECK(tlaxtrelom_geo_begin_frame(s, &view, 2) == TLAXTRELOM_GEO_OK);
    CHECK(tlaxtrelom_geo_collect(s, requests, 8) == 0);

    CHECK(slot_collision_loads_both_pages(asset.path) == 0);

    TempFile culled;
    TempFile galactic;
    TempFile legacy;
    CHECK(!culled.path.empty());
    CHECK(!galactic.path.empty());
    CHECK(!legacy.path.empty());
    CHECK(frustum_culls_offscreen_clusters(culled.path) == 0);
    CHECK(lod_distance_stays_double_at_galactic_anchor(galactic.path) == 0);
    CHECK(rejects_the_previous_asset_version(legacy.path) == 0);

    tlaxtrelom_geo_destroy(s);
    tlaxtrelom_geo_destroy(nullptr);

    std::printf("abi_smoke: OK (page_id=%llu, bytes=%llu)\n",
                static_cast<unsigned long long>(page_id),
                static_cast<unsigned long long>(stats.io_bytes));
    return 0;
}
