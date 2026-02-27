SHELL := /bin/bash

PYTHON ?= python3
PIP ?= pip
NPM ?= npm
CARGO ?= cargo

BACKEND_DIR := .
FRONTEND_DIR := src/frontend
RUST_TILE_DIR := src/tile-rust

.PHONY: help install install-python install-node install-rust install-all \
	dev-backend dev-frontend dev-rust-tile \
	build-frontend build-rust-tile fmt-rust \
	docker-build docker-up docker-down docker-logs \
	clean-node clean-python clean-docker

help:
	@echo "Targets:"
	@echo "  install-python   Install Python backend dependencies"
	@echo "  install-node     Install frontend dependencies"
	@echo "  install-rust     Fetch Rust tile-service dependencies"
	@echo "  install          Install all dependencies"
	@echo "  install-all      Alias for install"
	@echo "  dev-backend      Run FastAPI backend on :8080"
	@echo "  dev-frontend     Run Vite frontend on :5173"
	@echo "  dev-rust-tile    Run Rust tile service on :8090"
	@echo "  build-frontend   Build frontend assets"
	@echo "  build-rust-tile  Build rust tile service"
	@echo ""
	@echo "Docker targets:"
	@echo "  docker-build     Build all Docker images"
	@echo "  docker-up        Start all services with docker-compose"
	@echo "  docker-down      Stop all services"
	@echo "  docker-logs      Tail docker-compose logs"
	@echo "  clean-docker     Stop, remove containers, and images"

install: install-python install-node install-rust

install-all: install

install-python:
	$(PIP) install -e .[dev]

install-node:
	cd $(FRONTEND_DIR) && $(NPM) install

install-rust:
	cd $(RUST_TILE_DIR) && $(CARGO) fetch

dev-backend:
	uvicorn parquet_viewer_backend.main:app --host 0.0.0.0 --port 8080

dev-frontend:
	cd $(FRONTEND_DIR) && $(NPM) run dev

dev-rust-tile:
	cd $(RUST_TILE_DIR) && TILE_BIND=0.0.0.0:8090 $(CARGO) run

build-frontend:
	cd $(FRONTEND_DIR) && $(NPM) run build

build-rust-tile:
	cd $(RUST_TILE_DIR) && $(CARGO) build --release

fmt-rust:
	cd $(RUST_TILE_DIR) && $(CARGO) fmt

clean-node:
	rm -rf $(FRONTEND_DIR)/node_modules

clean-python:
	rm -rf .pytest_cache .ruff_cache .mypy_cache build dist *.egg-info

clean-docker:
	docker-compose down -v
	docker image rm -f parquet-viewer-backend parquet-viewer-frontend parquet-viewer-tile-service 2>/dev/null || true

docker-build:
	docker-compose build

docker-up:
	docker-compose up -d

docker-down:
	docker-compose down

docker-logs:
	docker-compose logs -f
