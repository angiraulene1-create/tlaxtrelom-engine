// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.
//
// cook_asset.cpp — Generador de assets cocinados TLXG v1 con geometría TLXM.

#include "../include/tlaxtrelom_geometry.h"

#include <zstd.h>

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include <fcntl.h>
#include <unistd.h>

namespace {

constexpr uint32_t kAssetMagic = 0x544C5847u; // "TLXG"
constexpr uint32_t kMeshMagic  = 0x544C584Du; // "TLXM"

struct AssetHeader {
    uint32_t magic;
    uint16_t version;
    uint16_t flags;
    uint32_t cluster_count;
    uint32_t page_count;
    uint32_t root_count;
    uint32_t reserved;
};
static_assert(sizeof(AssetHeader) == 24, "layout estable");

struct Cluster {
    float center[3];
    float radius;
    float lod_error;
    uint32_t page_local;
    uint32_t first_child;
    uint8_t child_count;
    uint8_t _pad[3];
};
static_assert(sizeof(Cluster) == 32, "layout estable");

struct PageDesc {
    uint64_t offset;
    uint32_t compressed_size;
    uint32_t raw_size;
};
static_assert(sizeof(PageDesc) == 16, "layout estable");

struct MeshHeader {
    uint32_t magic;
    uint32_t vertex_count;
    uint32_t index_count;
    uint32_t _pad;
};

struct MeshVertex {
    float position[3];
    float normal[3];
    float color[3];
};

bool write_all(int fd, const void* data, size_t size) {
    const auto* p = static_cast<const uint8_t*>(data);
    size_t done = 0;
    while (done < size) {
        const ssize_t n = ::write(fd, p + done, size - done);
        if (n <= 0) return false;
        done += static_cast<size_t>(n);
    }
    return true;
}

// Genera un prisma octaédrico brillante flotante centrado en (0, 3.2, 12)
std::vector<uint8_t> build_streamed_mesh() {
    std::vector<MeshVertex> vertices;
    std::vector<uint16_t> indices;

    const float cx = 0.0f;
    const float cy = 3.2f;
    const float cz = 12.0f;
    const float r = 1.0f;
    const float h = 1.6f;

    // Vértices del octaedro: punta superior, base de 4 puntas, punta inferior
    const float top[3] = {cx, cy + h, cz};
    const float bot[3] = {cx, cy - h, cz};
    const float mid[4][3] = {
        {cx + r, cy, cz},
        {cx, cy, cz + r},
        {cx - r, cy, cz},
        {cx, cy, cz - r},
    };

    const float gold[3] = {1.0f, 0.85f, 0.25f};
    const float cyan[3] = {0.15f, 0.90f, 0.95f};

    auto add_facet = [&](const float a[3], const float b[3], const float c[3], const float color[3]) {
        float ab[3] = {b[0] - a[0], b[1] - a[1], b[2] - a[2]};
        float ac[3] = {c[0] - a[0], c[1] - a[1], c[2] - a[2]};
        float nx = ab[1] * ac[2] - ab[2] * ac[1];
        float ny = ab[2] * ac[0] - ab[0] * ac[2];
        float nz = ab[0] * ac[1] - ab[1] * ac[0];
        float len = std::sqrt(nx * nx + ny * ny + nz * nz);
        if (len < 1e-6f) len = 1.0f;
        nx /= len; ny /= len; nz /= len;

        uint16_t base = static_cast<uint16_t>(vertices.size());
        vertices.push_back({{a[0], a[1], a[2]}, {nx, ny, nz}, {color[0], color[1], color[2]}});
        vertices.push_back({{b[0], b[1], b[2]}, {nx, ny, nz}, {color[0], color[1], color[2]}});
        vertices.push_back({{c[0], c[1], c[2]}, {nx, ny, nz}, {color[0], color[1], color[2]}});

        indices.push_back(base);
        indices.push_back(base + 1);
        indices.push_back(base + 2);
    };

    // Pirámide superior
    add_facet(top, mid[0], mid[1], gold);
    add_facet(top, mid[1], mid[2], cyan);
    add_facet(top, mid[2], mid[3], gold);
    add_facet(top, mid[3], mid[0], cyan);

    // Pirámide inferior
    add_facet(bot, mid[1], mid[0], cyan);
    add_facet(bot, mid[2], mid[1], gold);
    add_facet(bot, mid[3], mid[2], cyan);
    add_facet(bot, mid[0], mid[3], gold);

    MeshHeader hdr{};
    hdr.magic = kMeshMagic;
    hdr.vertex_count = static_cast<uint32_t>(vertices.size());
    hdr.index_count = static_cast<uint32_t>(indices.size());
    hdr._pad = 0;

    std::vector<uint8_t> payload(sizeof(MeshHeader) +
                                 vertices.size() * sizeof(MeshVertex) +
                                 indices.size() * sizeof(uint16_t));
    uint8_t* dst = payload.data();
    std::memcpy(dst, &hdr, sizeof(hdr)); dst += sizeof(hdr);
    std::memcpy(dst, vertices.data(), vertices.size() * sizeof(MeshVertex)); dst += vertices.size() * sizeof(MeshVertex);
    std::memcpy(dst, indices.data(), indices.size() * sizeof(uint16_t));

    return payload;
}

} // namespace

int main(int argc, char** argv) {
    std::string out_path = "assets/demo_cluster.tlxg";
    if (argc > 1) {
        out_path = argv[1];
    }

    auto payload = build_streamed_mesh();

    std::vector<uint8_t> compressed(ZSTD_compressBound(payload.size()));
    const size_t c_size = ZSTD_compress(compressed.data(), compressed.size(),
                                        payload.data(), payload.size(), 3);
    if (ZSTD_isError(c_size)) {
        std::fprintf(stderr, "error de compresión zstd: %s\n", ZSTD_getErrorName(c_size));
        return 1;
    }
    compressed.resize(c_size);

    AssetHeader header{};
    header.magic = kAssetMagic;
    header.version = 1;
    header.cluster_count = 1;
    header.page_count = 1;
    header.root_count = 1;

    Cluster cluster{};
    cluster.center[0] = 0.0f;
    cluster.center[1] = 3.2f;
    cluster.center[2] = 12.0f;
    cluster.radius = 2.5f;
    cluster.lod_error = 0.0f; // cluster hoja: siempre elegible cuando esté visible
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

    const int fd = ::open(out_path.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        std::perror("no se pudo abrir archivo de salida");
        return 1;
    }

    const bool ok = write_all(fd, &header, sizeof(header)) &&
                    write_all(fd, &cluster, sizeof(cluster)) &&
                    write_all(fd, &root, sizeof(root)) &&
                    write_all(fd, &page, sizeof(page)) &&
                    write_all(fd, compressed.data(), compressed.size());
    ::close(fd);

    if (!ok) {
        std::fprintf(stderr, "error escribiendo asset %s\n", out_path.c_str());
        return 1;
    }

    std::printf("Asset cocinado exitosamente: %s (%zu B comp, %zu B crudos)\n",
                out_path.c_str(), compressed.size(), payload.size());
    return 0;
}
