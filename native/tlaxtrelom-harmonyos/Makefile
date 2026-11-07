# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.

CXX ?= g++
CXXFLAGS ?= -std=c++20 -O2 -Wall -Wextra -Werror -fPIC -Iinclude

BUILD_DIR = build
LIB = $(BUILD_DIR)/libtlaxtrelom_harmonyos.so
TEST_BIN = $(BUILD_DIR)/harmonyos_smoke

.PHONY: all clean test

all: $(LIB) $(TEST_BIN)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(LIB): src/tlaxtrelom_harmonyos.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -shared -o $@ $<

$(TEST_BIN): tests/harmonyos_smoke.cpp $(LIB) | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -o $@ tests/harmonyos_smoke.cpp -L$(BUILD_DIR) -ltlaxtrelom_harmonyos -Wl,-rpath,'$$ORIGIN'

test: $(TEST_BIN)
	$(TEST_BIN)

clean:
	rm -rf $(BUILD_DIR)
