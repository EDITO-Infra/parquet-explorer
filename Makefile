.PHONY: check test ci dev-backend dev-frontend build-backend build-frontend docker docs docs-build docs-preview

check:
	cargo check --manifest-path src/backend/Cargo.toml

# Mirror .github/workflows/ci.yml so 'make ci' matches exactly what CI runs.
ci: backend-ci frontend-ci

backend-ci:
	cargo fmt --manifest-path src/backend/Cargo.toml --all -- --check
	cargo check --manifest-path src/backend/Cargo.toml --locked
	cargo test --manifest-path src/backend/Cargo.toml --locked
	cargo clippy --manifest-path src/backend/Cargo.toml --all-targets --locked -- -D warnings

frontend-ci:
	npm install --no-audit --no-fund --prefix src/frontend
	npm run build --prefix src/frontend

test:
	cargo test --manifest-path src/backend/Cargo.toml

build-backend:
	cargo build --release --manifest-path src/backend/Cargo.toml

dev-backend:
	RUST_LOG=info cargo run --manifest-path src/backend/Cargo.toml

dev-frontend:
	cd src/frontend && npm run dev

build-frontend:
	cd src/frontend && npm run build

docker:
	docker build -f deploy/Dockerfile -t parquet-explorer .
	docker run --rm -p 3000:80 parquet-explorer

docs:
	cd docs && npm run dev

docs-build:
	cd docs && npm run build

docs-preview:
	cd docs && npm run preview
