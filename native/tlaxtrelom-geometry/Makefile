# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
#
# Build de la isla C++ sin CMake (el entorno de referencia no lo trae).
# El runtime Rust la enlaza desde build/libtlaxtrelom_geometry.so.
#
#   make            compila la biblioteca
#   make test       compila y ejecuta el smoke test de la ABI C
#   make clean      borra build/
#
# Variables: CXX, CXXFLAGS, LDFLAGS, ZSTD_LIBS.

CXX      ?= g++
CXXSTD   ?= -std=c++20
CXXFLAGS ?= -O2 -g -fPIC -fvisibility=hidden -fvisibility-inlines-hidden
CXXFLAGS += $(CXXSTD) -Wall -Wextra -Wpedantic -Wconversion -Wshadow -D_FILE_OFFSET_BITS=64
CPPFLAGS += -Iinclude
LDFLAGS  ?=
ZSTD_LIBS ?= -lzstd

BUILD    := build
LIB      := $(BUILD)/libtlaxtrelom_geometry.so
SRC      := src/geometry_streamer.cpp
HDR      := include/tlaxtrelom_geometry.h
OBJ      := $(BUILD)/geometry_streamer.o
TEST_BIN := $(BUILD)/abi_smoke
TEST_SRC := tests/abi_smoke.cpp
COOKER_BIN := $(BUILD)/cook_asset
COOKER_SRC := tools/cook_asset.cpp

.PHONY: all test cook clean

all: $(LIB) $(COOKER_BIN)

$(BUILD):
	mkdir -p $(BUILD)

$(OBJ): $(SRC) $(HDR) | $(BUILD)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c $(SRC) -o $@

$(LIB): $(OBJ)
	$(CXX) -shared $(LDFLAGS) -o $@ $^ $(ZSTD_LIBS)

$(TEST_BIN): $(TEST_SRC) $(HDR) $(LIB)
	$(CXX) $(CPPFLAGS) $(CXXSTD) -O2 -g -Wall -Wextra -Wpedantic \
		$(TEST_SRC) -L$(BUILD) -ltlaxtrelom_geometry $(ZSTD_LIBS) \
		-Wl,-rpath,'$$ORIGIN' -o $@

$(COOKER_BIN): $(COOKER_SRC) $(HDR)
	$(CXX) $(CPPFLAGS) $(CXXSTD) -O2 -g -Wall -Wextra \
		$(COOKER_SRC) $(ZSTD_LIBS) -o $@

cook: $(COOKER_BIN)
	mkdir -p ../../assets
	./$(COOKER_BIN) ../../assets/demo_cluster.tlxg

test: $(TEST_BIN)
	./$(TEST_BIN)

clean:
	rm -rf $(BUILD)
