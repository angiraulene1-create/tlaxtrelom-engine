# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.

CC ?= gcc
CFLAGS ?= -std=c11 -O3 -Wall -Wextra -Werror -fPIC -Iinclude

BUILD_DIR = build
LIB = $(BUILD_DIR)/libtlaxtrelom_spatial.so
TEST_BIN = $(BUILD_DIR)/spatial_smoke

.PHONY: all clean test

all: $(LIB) $(TEST_BIN)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(LIB): src/tlaxtrelom_spatial.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -shared -o $@ $< -lm

$(TEST_BIN): tests/spatial_smoke.c $(LIB) | $(BUILD_DIR)
	$(CC) $(CFLAGS) -o $@ tests/spatial_smoke.c -L$(BUILD_DIR) -ltlaxtrelom_spatial -Wl,-rpath,'$$ORIGIN' -lm

test: $(TEST_BIN)
	$(TEST_BIN)

clean:
	rm -rf $(BUILD_DIR)
