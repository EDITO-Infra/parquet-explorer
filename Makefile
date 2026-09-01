.PHONY: check test dev-backend dev-frontend build-backend build-frontend docker docs docs-build docs-preview

check:
	cargo check --manifest-path src/backend/Cargo.toml

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
	docker compose up --build

docs:
	cd docs && npm run dev

docs-build:
	cd docs && npm run build

docs-preview:
	cd docs && npm run preview
