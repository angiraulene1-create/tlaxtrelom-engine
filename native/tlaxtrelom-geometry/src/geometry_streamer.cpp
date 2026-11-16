// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
/*
 * geometry_streamer.cpp — streaming de geometría de alta densidad (C++).
 *
 * Modelo:
 *   Malla → clusters de 128 triángulos → DAG jerárquico (LOD) → páginas.
 *   El runtime recorre el DAG con el error proyectado en píxeles y pide las
 *   páginas que faltan. La isla C++ hace IO + descompresión; Rust sube a GPU.
 *
 * Por qué C++ aquí:
 *   - `pread` + zstd + hilos de IO con control fino de afinidad.
 *   - Código de terceros (compresores, códecs de malla) ya existente.
 *   - El DAG y el recorrido son lógica pura, sin dependencias de wgpu.
 *
 * Lo que NO hace:
 *   - No toca Vulkan/DX12/wgpu.
 *   - No conoce entidades, materiales ni shaders.
 *   - No libera memoria de GPU (eso es del residency manager en Rust).
 */

#include "tlaxtrelom_geometry.h"

#include <zstd.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <deque>
#include <memory>
#include <mutex>
#include <new>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <io.h>
#else
#  include <fcntl.h>
#  include <sys/stat.h>
#  include <unistd.h>
#endif

namespace {

constexpr uint32_t kAssetMagic = 0x544C5847u; // "TLXG" — Tlaxtrelom Geometry
// v2: la cabecera gana `origin[3]` en `double` y los `center` de los clusters
// pasan a ser relativos a ese origen. Un asset cocinado con v1 se rechaza.
constexpr uint32_t kAssetVersion = 2;

// La ABI asume un `TlaxtrelomGeoView` de 120 bytes: 23 campos de 4 B (92, con
// padding implícito hasta 96) + `double origin[3]`. Rust espeja este layout en
// `geometry.rs`; si uno de los dos cambia, el otro debe cambiar con él.
static_assert(sizeof(TlaxtrelomGeoView) == 120, "layout de TlaxtrelomGeoView");

// ─────────────────────────────────────────────────────────────────────────────
// Estado de página
// ─────────────────────────────────────────────────────────────────────────────

enum PageState : uint8_t {
    kUnloaded = 0,
    kLoading = 1,
    kReady = 2,
    kResident = 3,
    kEvicting = 4,
};

// 64 bytes: una línea de caché por página. Evita false sharing entre hilos.
struct alignas(64) PageSlot {
    std::atomic<uint8_t> state{kUnloaded};
    std::atomic<uint64_t> last_used{0};
    std::atomic<uint64_t> tag{0}; // page_id actual (detecta colisiones)
    uint32_t size{0};
    uint32_t _pad{0};
};

struct Cluster {
    float center[3];       // relativo al `origin` del asset (v2)
    float radius;
    float lod_error;     // error en unidades de mundo al usar este cluster
    uint32_t page_local; // índice de página dentro del asset
    uint32_t first_child; // índice del primer hijo en el vector
    uint8_t child_count;
    uint8_t _pad[3];
};
static_assert(sizeof(Cluster) == 32, "layout estable para el cooker");

struct PageDesc {
    uint64_t offset;
    uint32_t compressed_size;
    uint32_t raw_size;
};
static_assert(sizeof(PageDesc) == 16, "layout estable para el cooker");

struct AssetHeader {
    uint32_t magic;
    uint16_t version;
    uint16_t flags;
    uint32_t cluster_count;
    uint32_t page_count;
    uint32_t root_count;
    uint32_t reserved;
    // Dónde está el asset en el mundo, en metros y `double`. Los clusters son
    // `float` *relativos* a este punto: un centro absoluto en `float` no puede
    // vivir a 1e11 m del origen global sin perder varios kilómetros de ULP, y
    // el LOD se calcula contra esa distancia.
    double origin[3];
};
static_assert(sizeof(AssetHeader) == 48, "layout estable para el cooker");

struct Asset {
    uint32_t id{0};
    int fd{-1};
    double origin[3]{0.0, 0.0, 0.0};
    std::vector<Cluster> clusters;
    std::vector<uint32_t> roots;
    std::vector<PageDesc> pages;
};

struct ReadyPage {
    uint64_t page_id;
    std::vector<uint8_t> bytes;
};

inline void platform_close(int fd) {
    if (fd >= 0) {
#if defined(_WIN32)
        ::_close(fd);
#else
        ::close(fd);
#endif
    }
}

inline int platform_open_readonly(const char* path) {
#if defined(_WIN32)
    return ::_open(path, _O_RDONLY | _O_BINARY);
#elif defined(O_CLOEXEC)
    return ::open(path, O_RDONLY | O_CLOEXEC);
#else
    return ::open(path, O_RDONLY);
#endif
}

struct FdGuard {
    int fd{-1};
    ~FdGuard() {
        platform_close(fd);
    }
};

inline uint32_t local_of(uint64_t pid) { return uint32_t(pid & 0xFFFFFFFFu); }

bool pread_all(int fd, void* dst, size_t n, uint64_t offset) {
    auto* p = static_cast<uint8_t*>(dst);
    size_t done = 0;
    while (done < n) {
#if defined(_WIN32)
        HANDLE hFile = reinterpret_cast<HANDLE>(::_get_osfhandle(fd));
        if (hFile == INVALID_HANDLE_VALUE) return false;
        OVERLAPPED ov{};
        uint64_t cur_offset = offset + done;
        ov.Offset = static_cast<DWORD>(cur_offset & 0xFFFFFFFF);
        ov.OffsetHigh = static_cast<DWORD>((cur_offset >> 32) & 0xFFFFFFFF);
        DWORD bytesRead = 0;
        DWORD toRead = static_cast<DWORD>(std::min<size_t>(n - done, 0xFFFFFFFFu));
        if (!::ReadFile(hFile, p + done, toRead, &bytesRead, &ov) || bytesRead == 0) {
            return false;
        }
        done += static_cast<size_t>(bytesRead);
#else
        const ssize_t r = ::pread(fd, p + done, n - done, off_t(offset + done));
        if (r <= 0) return false;
        done += size_t(r);
#endif
    }
    return true;
}

// ─────────────────────────────────────────────────────────────────────────────
// Frustum
// ─────────────────────────────────────────────────────────────────────────────
//
// Se extrae una vez por frame de `view_proj` (column-major, `m[col*4 + row]`) y
// se prueba contra la esfera de cada cluster. Sin esto, en un mundo grande el
// DAG se recorre completo y se piden páginas de clusters a espalda de la cámara.
//
// Convención: clip left-handed con profundidad 0..1 (wgpu/D3D), la misma de
// `xr::perspective`. Los planos salen de las filas de la matriz (Gribb-Hartmann):
//   izq = f0+f3, der = f3-f0, inf = f1+f3, sup = f3-f1,
//   cer = f2 (z >= 0 tras el sesgo de near), lej = f3-f2.
struct Frustum {
    // Cada plano es (nx, ny, nz, d) con (nx,ny,nz) unitario; dentro <=>
    // dot(n, p) + d >= 0.
    double planes[6][4]{};
    // Falso si la matriz no representa un frustum (todo ceros, NaN, o un plano
    // degenerado). Mejor no podar que vaciar la escena por una matriz rara.
    bool valid{false};
};

inline void plane_from(double* out, const double* a, const double* b, double sign) {
    for (int i = 0; i < 4; ++i) out[i] = b[i] + sign * a[i];
}

Frustum make_frustum(const TlaxtrelomGeoView& v) {
    Frustum f{};
    double row[4][4];
    for (int r = 0; r < 4; ++r) {
        for (int c = 0; c < 4; ++c) {
            row[r][c] = double(v.view_proj[c * 4 + r]);
        }
    }
    const double* f0 = row[0];
    const double* f1 = row[1];
    const double* f2 = row[2];
    const double* f3 = row[3];

    plane_from(f.planes[0], f0, f3, 1.0);   // izquierda
    plane_from(f.planes[1], f0, f3, -1.0);  // derecha
    plane_from(f.planes[2], f1, f3, 1.0);   // abajo
    plane_from(f.planes[3], f1, f3, -1.0);  // arriba
    for (int i = 0; i < 4; ++i) f.planes[4][i] = f2[i]; // cercano
    plane_from(f.planes[5], f2, f3, -1.0);  // lejano

    for (auto& plane : f.planes) {
        const double len = std::sqrt(plane[0] * plane[0] + plane[1] * plane[1] +
                                     plane[2] * plane[2]);
        if (!(len > 1e-12) || !std::isfinite(len)) return Frustum{};
        for (double& c : plane) {
            c /= len;
            if (!std::isfinite(c)) return Frustum{};
        }
    }
    f.valid = true;
    return f;
}

// Test esfera-vs-frustum sobre coordenadas locales de render (offset respecto
// del origen flotante). La aritmética es `double`: el radio es `float`, pero un
// centro de sector a 1e11 m no cabe en `float` sin mover kilómetros.
inline bool frustum_visible(const Frustum& f, const double center[3], double radius) {
    if (!f.valid) return true;
    for (const auto& plane : f.planes) {
        const double d = plane[0] * center[0] + plane[1] * center[1] +
                         plane[2] * center[2] + plane[3];
        if (d < -radius) return false;
    }
    return true;
}

} // namespace

// ─────────────────────────────────────────────────────────────────────────────
// Estado del streamer (definición del handle opaco)
// ─────────────────────────────────────────────────────────────────────────────

struct TlaxtrelomGeoStreamer {
    TlaxtrelomGeoConfig cfg{};
    std::string cache_dir;
    TlaxtrelomGeoView view{};
    // Derivado de `view.view_proj` en `begin_frame`; no se recalcula por cluster.
    Frustum frustum{};
    uint64_t frame_id{0};

    std::unique_ptr<PageSlot[]> slots;
    uint32_t slot_count{0};

    std::vector<std::unique_ptr<Asset>> assets;

    std::atomic<uint64_t> stat_pages_resident{0};
    std::atomic<uint64_t> stat_page_faults{0};
    std::atomic<uint64_t> stat_clusters_visible{0};
    std::atomic<uint64_t> stat_clusters_culled{0};
    std::atomic<uint64_t> stat_io_bytes{0};
    std::atomic<uint32_t> stat_io_queue{0};

    // Cola de peticiones (productor: render; consumidor: IO). El mutex es
    // aceptable porque se toca una vez por página faltante, no por frame.
    std::mutex req_mu;
    std::condition_variable req_cv;
    std::deque<TlaxtrelomGeoPageRequest> requests;
    bool stop{false};

    std::vector<std::thread> io_threads;

    // Páginas descomprimidas + buffers en vuelo hasta ack_upload.
    std::mutex ready_mu;
    std::deque<ReadyPage> ready;
    std::unordered_map<uint64_t, std::vector<uint8_t>> in_flight;
};

namespace {

inline PageSlot& slot_for(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    // Hash multiplicativo (Fibonacci) sobre el page_id completo. Con
    // `page_id % slot_count` todas las páginas locales iguales de assets
    // distintos caían en el mismo slot, porque `asset_id << 32` es múltiplo de
    // cualquier potencia de dos. Se usan los bits altos del producto para
    // evitar que los bits bajos de `page_local` dominen el índice.
    const uint64_t mixed = page_id * 0x9E3779B97F4A7C15ull;
    return s->slots[(mixed >> 32) % s->slot_count];
}

Asset* find_asset(TlaxtrelomGeoStreamer* s, uint32_t id) {
    for (auto& a : s->assets) {
        if (a->id == id) return a.get();
    }
    return nullptr;
}

void mark_failed(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    PageSlot& slot = slot_for(s, page_id);
    // El slot pudo reutilizarse para otra página (colisión de hash) mientras
    // esta petición estaba en vuelo: no se debe desalojar a la nueva.
    if (slot.tag.load(std::memory_order_relaxed) == page_id) {
        slot.state.store(kUnloaded, std::memory_order_release);
    }
}

// Centro del cluster en coordenadas locales de render: el origen del sector
// menos el origen flotante de la vista. En `double`, porque la resta de dos
// posiciones a 1e11 m no cabe en `float` (ULP ≈ 8 km).
inline void render_center(const Asset& a, const Cluster& c,
                          const TlaxtrelomGeoView& v, double out[3]) {
    for (int i = 0; i < 3; ++i) {
        out[i] = a.origin[i] + double(c.center[i]) - v.origin[i];
    }
}

// Error proyectado en píxeles: (error_mundo * alto_pantalla) /
// (2 * distancia * tan(fov/2)). Si supera el budget, hay que bajar de LOD.
// `center` es el centro en coordenadas locales de render (ver `render_center`).
// La distancia se acumula en `double`: con coordenadas absolutas `float` dos
// clusters separados cientos de metros a 1e12 m daban la misma distancia y el
// DAG colapsaba al LOD más burdo.
double screen_error(const Cluster& c, const TlaxtrelomGeoView& v, const double center[3]) {
    double sum = 0.0;
    for (int i = 0; i < 3; ++i) {
        const double d = center[i] - double(v.camera[i]);
        sum += d * d;
    }
    const double dist = std::sqrt(sum);
    const double half_fov = std::max(0.01, std::tan(double(v.fov_y_rad) * 0.5));
    return (double(c.lod_error) * double(v.screen_height)) /
           (2.0 * std::max(dist, 0.001) * half_fov);
}

bool request_page(TlaxtrelomGeoStreamer* s, uint32_t asset_id, uint64_t page_id,
                  uint32_t lod, uint32_t priority) {
    PageSlot& slot = slot_for(s, page_id);

    // El mapeo directo puede asignar el mismo slot a dos páginas distintas. Si
    // el slot ya está residente con otro `tag`, hay que desalojar la página
    // anterior; si no, la CAS desde `kUnloaded` falla para siempre y la página
    // colisionada nunca llega a cargarse.
    uint8_t expected = slot.state.load(std::memory_order_acquire);
    if (expected == kResident &&
        slot.tag.load(std::memory_order_relaxed) != page_id) {
        if (!slot.state.compare_exchange_strong(expected, kEvicting,
                                                std::memory_order_acq_rel)) {
            return false; // otro hilo la cambió; se reintenta el próximo frame
        }
        s->stat_pages_resident.fetch_sub(1, std::memory_order_relaxed);
    }

    const uint8_t from = slot.state.load(std::memory_order_acquire);
    if (from == kEvicting) {
        // Solo el hilo de render pide páginas (contrato de `collect`), así que
        // reclamar un slot ya desalojado es seguro.
        slot.state.store(kLoading, std::memory_order_release);
    } else {
        expected = kUnloaded;
        if (!slot.state.compare_exchange_strong(expected, kLoading,
                                                std::memory_order_acq_rel)) {
            return false; // ya está cargando o residente
        }
    }
    slot.tag.store(page_id, std::memory_order_relaxed);

    bool queued = false;
    {
        std::lock_guard<std::mutex> lk(s->req_mu);
        if (s->requests.size() < 8192) {
            s->requests.push_back(
                TlaxtrelomGeoPageRequest{page_id, asset_id, lod, priority, 0});
            queued = true;
        }
    }
    if (!queued) {
        // Backpressure: se revierte para reintentar el próximo frame.
        slot.state.store(kUnloaded, std::memory_order_release);
        return false;
    }
    s->stat_io_queue.fetch_add(1, std::memory_order_relaxed);
    s->req_cv.notify_one();
    return true;
}

void traverse(TlaxtrelomGeoStreamer* s, const Asset& a, uint32_t cluster_index,
              uint32_t& out_n, TlaxtrelomGeoPageRequest* out, uint32_t cap,
              uint32_t depth) {
    if (depth > 64 || cluster_index >= a.clusters.size()) return;
    const Cluster& c = a.clusters[cluster_index];

    double center[3];
    render_center(a, c, s->view, center);
    if (!frustum_visible(s->frustum, center, double(c.radius))) {
        // Fuera de vista: no se pide ni a él ni a sus hijos, que están dentro
        // de su esfera contenedora.
        s->stat_clusters_culled.fetch_add(1, std::memory_order_relaxed);
        return;
    }

    const double err = screen_error(c, s->view, center);
    const bool leaf = c.child_count == 0;

    if (err <= double(s->view.pixel_error_budget) || leaf) {
        s->stat_clusters_visible.fetch_add(1, std::memory_order_relaxed);

        const uint64_t pid = (uint64_t(a.id) << 32) | c.page_local;
        PageSlot& slot = slot_for(s, pid);
        const uint8_t st = slot.state.load(std::memory_order_acquire);

        if (st != kResident ||
            slot.tag.load(std::memory_order_relaxed) != pid) {
            const uint32_t prio = uint32_t(std::min(err, 1.0e6)) + 1;
            // Solo se contabiliza la petición si realmente se encoló: una
            // página ya en vuelo no es un page fault nuevo.
            if (request_page(s, a.id, pid, c.page_local, prio)) {
                s->stat_page_faults.fetch_add(1, std::memory_order_relaxed);
                if (out_n < cap) {
                    out[out_n++] = TlaxtrelomGeoPageRequest{pid, a.id, c.page_local,
                                                      prio, 0};
                }
            }
        } else {
            slot.last_used.store(s->frame_id, std::memory_order_relaxed);
        }
        return;
    }

    // Nodo interno visible pero con error alto: se baja a los hijos.
    s->stat_clusters_culled.fetch_add(1, std::memory_order_relaxed);
    for (uint32_t i = 0; i < c.child_count; ++i) {
        traverse(s, a, c.first_child + i, out_n, out, cap, depth + 1);
    }
}

void io_worker(TlaxtrelomGeoStreamer* s) {
    for (;;) {
        TlaxtrelomGeoPageRequest req{};
        {
            std::unique_lock<std::mutex> lk(s->req_mu);
            s->req_cv.wait(lk, [s] {
                return s->stop || !s->requests.empty();
            });
            if (s->stop && s->requests.empty()) return;
            req = s->requests.front();
            s->requests.pop_front();
            s->stat_io_queue.fetch_sub(1, std::memory_order_relaxed);
        }

        Asset* a = find_asset(s, req.asset_id);
        if (!a) {
            mark_failed(s, req.page_id);
            continue;
        }
        const uint32_t local = local_of(req.page_id);
        if (local >= a->pages.size()) {
            mark_failed(s, req.page_id);
            continue;
        }
        const PageDesc& pd = a->pages[local];
        if (pd.compressed_size == 0 || pd.raw_size == 0) {
            mark_failed(s, req.page_id);
            continue;
        }

        std::vector<uint8_t> compressed(pd.compressed_size);
        if (!pread_all(a->fd, compressed.data(), compressed.size(),
                       pd.offset)) {
            mark_failed(s, req.page_id);
            continue;
        }

        std::vector<uint8_t> raw(pd.raw_size);
        const size_t decoded =
            ZSTD_decompress(raw.data(), raw.size(), compressed.data(),
                            compressed.size());
        if (ZSTD_isError(decoded) || decoded != raw.size()) {
            mark_failed(s, req.page_id);
            continue;
        }

        s->stat_io_bytes.fetch_add(pd.compressed_size,
                                   std::memory_order_relaxed);

        {
            std::lock_guard<std::mutex> lk(s->ready_mu);
            if (s->ready.size() < s->cfg.max_ready_pages) {
                s->ready.push_back(ReadyPage{req.page_id, std::move(raw)});
            } else {
                // El render no está drenando: se descarta y se reintenta.
                mark_failed(s, req.page_id);
            }
        }
    }
}

} // namespace

// ─────────────────────────────────────────────────────────────────────────────
// ABI
// ─────────────────────────────────────────────────────────────────────────────

extern "C" {

TLAXTRELOM_GEO_EXPORT uint32_t tlaxtrelom_geo_abi_version(void) { return 2; }

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_create(const TlaxtrelomGeoConfig* cfg, TlaxtrelomGeoStreamer** out) {
    if (!cfg || !out) return TLAXTRELOM_GEO_ERR_INVALID;
    *out = nullptr;

    TlaxtrelomGeoStreamer* s = nullptr;
    try {
        s = new TlaxtrelomGeoStreamer();
        s->cfg = *cfg;
        if (cfg->cache_dir) s->cache_dir = cfg->cache_dir;
        if (s->cfg.max_resident_pages == 0) s->cfg.max_resident_pages = 4096;
        if (s->cfg.io_threads == 0) s->cfg.io_threads = 1;
        if (s->cfg.max_ready_pages == 0) s->cfg.max_ready_pages = 256;

        s->slot_count = s->cfg.max_resident_pages;
        s->slots = std::make_unique<PageSlot[]>(s->slot_count);

        for (uint32_t i = 0; i < s->cfg.io_threads; ++i) {
            s->io_threads.emplace_back(io_worker, s);
        }
        *out = s;
        return TLAXTRELOM_GEO_OK;
    } catch (const std::bad_alloc&) {
        if (s) {
            {
                std::lock_guard<std::mutex> lk(s->req_mu);
                s->stop = true;
            }
            s->req_cv.notify_all();
            for (auto& t : s->io_threads) {
                if (t.joinable()) t.join();
            }
            delete s;
        }
        return TLAXTRELOM_GEO_ERR_OOM;
    } catch (...) {
        if (s) {
            {
                std::lock_guard<std::mutex> lk(s->req_mu);
                s->stop = true;
            }
            s->req_cv.notify_all();
            for (auto& t : s->io_threads) {
                if (t.joinable()) t.join();
            }
            delete s;
        }
        return TLAXTRELOM_GEO_ERR_STATE;
    }
}

TLAXTRELOM_GEO_EXPORT void tlaxtrelom_geo_destroy(TlaxtrelomGeoStreamer* s) {
    if (!s) return;
    {
        std::lock_guard<std::mutex> lk(s->req_mu);
        s->stop = true;
    }
    s->req_cv.notify_all();
    for (auto& t : s->io_threads) {
        if (t.joinable()) t.join();
    }
    for (auto& a : s->assets) {
        platform_close(a->fd);
    }
    delete s;
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_register_asset(TlaxtrelomGeoStreamer* s, uint32_t asset_id,
                        const char* path) {
    if (!s || !path) return TLAXTRELOM_GEO_ERR_INVALID;
    try {
        FdGuard fd;
        fd.fd = platform_open_readonly(path);
        if (fd.fd < 0) return TLAXTRELOM_GEO_ERR_IO;

        AssetHeader h{};
        if (!pread_all(fd.fd, &h, sizeof(h), 0)) return TLAXTRELOM_GEO_ERR_IO;
        if (h.magic != kAssetMagic || h.version != kAssetVersion) {
            return TLAXTRELOM_GEO_ERR_FORMAT;
        }

        auto a = std::make_unique<Asset>();
        a->id = asset_id;
        for (int i = 0; i < 3; ++i) {
            // Un anclaje no finito corrompería cada distancia del DAG de forma
            // silenciosa: se rechaza el asset, no se propaga.
            if (!std::isfinite(h.origin[i])) return TLAXTRELOM_GEO_ERR_FORMAT;
            a->origin[i] = h.origin[i];
        }
        a->clusters.resize(h.cluster_count);
        a->roots.resize(h.root_count);
        a->pages.resize(h.page_count);

        uint64_t off = sizeof(AssetHeader);
        if (!pread_all(fd.fd, a->clusters.data(),
                       a->clusters.size() * sizeof(Cluster), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }
        off += a->clusters.size() * sizeof(Cluster);
        if (!pread_all(fd.fd, a->roots.data(),
                       a->roots.size() * sizeof(uint32_t), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }
        off += a->roots.size() * sizeof(uint32_t);
        if (!pread_all(fd.fd, a->pages.data(),
                       a->pages.size() * sizeof(PageDesc), off)) {
            return TLAXTRELOM_GEO_ERR_IO;
        }

        a->fd = fd.fd;
        fd.fd = -1; // ownership transferido al asset
        s->assets.push_back(std::move(a));
        return TLAXTRELOM_GEO_OK;
    } catch (const std::bad_alloc&) {
        return TLAXTRELOM_GEO_ERR_OOM;
    } catch (...) {
        return TLAXTRELOM_GEO_ERR_STATE;
    }
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_begin_frame(TlaxtrelomGeoStreamer* s, const TlaxtrelomGeoView* view,
                     uint64_t frame_id) {
    if (!s || !view) return TLAXTRELOM_GEO_ERR_INVALID;
    for (double o : view->origin) {
        if (!std::isfinite(o)) return TLAXTRELOM_GEO_ERR_INVALID;
    }
    // Solo el hilo de render llama a esta función; no hay carrera con
    // `collect`, que se ejecuta inmediatamente después en el mismo hilo.
    s->view = *view;
    s->frustum = make_frustum(*view);
    s->frame_id = frame_id;
    return TLAXTRELOM_GEO_OK;
}

TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_collect(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoPageRequest* out,
                 uint32_t capacity) {
    if (!s || !out || capacity == 0) return 0;
    uint32_t n = 0;
    for (auto& a : s->assets) {
        for (uint32_t root : a->roots) {
            traverse(s, *a, root, n, out, capacity, 0);
        }
    }
    // Prioridad descendente: el residency manager de Rust sube primero lo
    // que más se va a ver. `std::sort` no asigna memoria.
    std::sort(out, out + n, [](const TlaxtrelomGeoPageRequest& a,
                               const TlaxtrelomGeoPageRequest& b) {
        return a.priority > b.priority;
    });
    return n;
}

TLAXTRELOM_GEO_EXPORT uint32_t
tlaxtrelom_geo_drain_ready(TlaxtrelomGeoStreamer* s, TlaxtrelomGeoReadyPage* out,
                     uint32_t capacity) {
    if (!s || !out || capacity == 0) return 0;
    try {
        std::lock_guard<std::mutex> lk(s->ready_mu);
        uint32_t n = 0;
        while (n < capacity && !s->ready.empty()) {
            ReadyPage rp = std::move(s->ready.front());
            s->ready.pop_front();

            // El buffer vive en `in_flight` hasta ack_upload. Mover el vector
            // al mapa no mueve su heap: `data()` queda estable.
            auto [it, inserted] = s->in_flight.try_emplace(
                rp.page_id, std::move(rp.bytes));
            (void)inserted;
            out[n++] = TlaxtrelomGeoReadyPage{rp.page_id, it->second.data(),
                                        uint32_t(it->second.size()), 0, 0};
        }
        return n;
    } catch (...) {
        return 0; // el render reintenta el próximo frame
    }
}

TLAXTRELOM_GEO_EXPORT TlaxtrelomGeoResult
tlaxtrelom_geo_ack_upload(TlaxtrelomGeoStreamer* s, uint64_t page_id) {
    if (!s) return TLAXTRELOM_GEO_ERR_INVALID;
    std::lock_guard<std::mutex> lk(s->ready_mu);
    auto it = s->in_flight.find(page_id);
    if (it == s->in_flight.end()) return TLAXTRELOM_GEO_ERR_STATE;
    s->in_flight.erase(it);

    PageSlot& slot = slot_for(s, page_id);
    slot.state.store(kResident, std::memory_order_release);
    slot.last_used.store(s->frame_id, std::memory_order_relaxed);
    s->stat_pages_resident.fetch_add(1, std::memory_order_relaxed);
    return TLAXTRELOM_GEO_OK;
}

TLAXTRELOM_GEO_EXPORT void
tlaxtrelom_geo_get_stats(const TlaxtrelomGeoStreamer* s, TlaxtrelomGeoStats* out) {
    if (!s || !out) return;
    out->pages_resident =
        s->stat_pages_resident.load(std::memory_order_relaxed);
    out->pages_pending = s->stat_io_queue.load(std::memory_order_relaxed);
    out->bytes_resident = 0; // lo mantiene el residency manager de Rust
    out->clusters_visible =
        s->stat_clusters_visible.load(std::memory_order_relaxed);
    out->clusters_culled =
        s->stat_clusters_culled.load(std::memory_order_relaxed);
    out->page_faults = s->stat_page_faults.load(std::memory_order_relaxed);
    out->io_bytes = s->stat_io_bytes.load(std::memory_order_relaxed);
    out->io_queue_depth = uint32_t(out->pages_pending);
    out->_pad = 0;
}

} // extern "C"
