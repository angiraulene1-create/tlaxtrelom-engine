# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
#
# Fachada de conveniencia. La fuente de verdad es `cargo xtask`; este Makefile
# solo evita recordar los comandos largos.

.PHONY: help build release test test-all ci fmt fmt-check clippy check \
        native native-test spatial harmonyos shaders dotnet relay python lua node docs selftest probe run clean

help:
	@echo "Tlaxtrelom Engine — objetivos disponibles"
	@echo ""
	@echo "  make build        build debug del runtime"
	@echo "  make release      build release (optimizado, LTO)"
	@echo "  make test         tests Rust (features por defecto)"
	@echo "  make test-all     tests Rust + isla C++"
	@echo "  make ci           matriz completa (fmt, clippy, C++, Rust, Python, Lua, Node, docs)"
	@echo "  make fmt          formatea el workspace"
	@echo "  make fmt-check    verifica formato sin escribir"
	@echo "  make clippy       Clippy con -D warnings"
	@echo "  make check        cargo check --workspace --all-targets"
	@echo "  make native       compila la isla C++ (Makefile)"
	@echo "  make native-test  smoke test de la ABI C++"
	@echo "  make python       tests de las herramientas Python"
	@echo "  make lua          tests del runtime de scripting Lua"
	@echo "  make node         tests de las herramientas TypeScript"
	@echo "  make docs         valida enlaces locales de la documentación"
	@echo "  make selftest     selftest determinista sin GPU"
	@echo "  make probe        enumera adaptadores wgpu"
	@echo "  make run          ejecuta el host de escritorio"
	@echo "  make clean        limpia artefactos de build"

build:
	cargo build --workspace

release:
	cargo build --workspace --release

test:
	cargo xtask test

test-all:
	cargo xtask test-all

ci:
	cargo xtask ci

fmt:
	cargo xtask fmt

fmt-check:
	cargo xtask fmt-check

clippy:
	cargo xtask clippy

check:
	cargo xtask check

native:
	cargo xtask native

native-test:
	cargo xtask native-test

spatial:
	cargo xtask spatial

harmonyos:
	cargo xtask harmonyos

shaders:
	cargo xtask shaders

dotnet:
	cargo xtask dotnet

relay:
	cargo xtask relay

python:
	cargo xtask python

lua:
	cargo xtask lua

node:
	cargo xtask node

docs:
	cargo xtask docs

selftest:
	cargo run -q -p tlaxtrelom-core --bin tlaxtrelom -- --selftest

probe:
	cargo run -q -p tlaxtrelom-core --bin tlaxtrelom -- --probe

run:
	cargo run -p tlaxtrelom-core --bin tlaxtrelom

clean:
	cargo xtask clean
