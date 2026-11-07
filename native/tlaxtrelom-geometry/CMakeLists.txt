# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
cmake_minimum_required(VERSION 3.25)
project(tlaxtrelom_geometry LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(tlaxtrelom_geometry SHARED
    src/geometry_streamer.cpp
)

target_include_directories(tlaxtrelom_geometry
    PUBLIC include
)

find_package(zstd REQUIRED)
target_link_libraries(tlaxtrelom_geometry PRIVATE zstd::libzstd)

# El runtime enlaza contra esta isla; los símbolos internos no se exportan.
set_target_properties(tlaxtrelom_geometry PROPERTIES
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN ON
)

target_compile_definitions(tlaxtrelom_geometry PRIVATE _FILE_OFFSET_BITS=64)

target_compile_options(tlaxtrelom_geometry PRIVATE
    -Wall -Wextra -Wpedantic -Wconversion -Wshadow
)

# Sanitizers en CI (no en release).
option(TLAXTRELOM_SANITIZE "Build with ASan/UBSan" OFF)
if(TLAXTRELOM_SANITIZE)
    target_compile_options(tlaxtrelom_geometry PRIVATE -fsanitize=address,undefined -fno-omit-frame-pointer)
    target_link_options(tlaxtrelom_geometry PRIVATE -fsanitize=address,undefined)
endif()
