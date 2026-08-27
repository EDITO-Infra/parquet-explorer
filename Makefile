.PHONY: check test dev-backend dev-frontend build-backend build-frontend docker

check:
	cargo check --manifest-path src/server/Cargo.toml

test:
	cargo test --manifest-path src/server/Cargo.toml

build-backend:
	cargo build --release --manifest-path src/server/Cargo.toml

dev-backend:
	RUST_LOG=info cargo run --manifest-path src/server/Cargo.toml

dev-frontend:
	cd src/frontend && npm run dev

build-frontend:
	cd src/frontend && npm run build

docker:
	docker compose up --build
