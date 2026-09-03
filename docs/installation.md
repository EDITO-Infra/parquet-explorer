# Installation and local setup

## Prerequisites

- Rust and Cargo **1.88+** — use the latest stable Rust when possible.
- Node.js **20.19+ or 22.12+** with npm — use the latest LTS release when possible.
- Optional: Docker with Compose support.

## Install

From the repository root:

```bash
rustup update stable
cargo fetch --manifest-path src/backend/Cargo.toml --locked
npm ci --prefix src/frontend
```

To work on this documentation site, also run:

```bash
npm ci --prefix docs
```

## Run

Start the backend and frontend in separate terminals:

```bash
cargo run --manifest-path src/backend/Cargo.toml
npm run dev --prefix src/frontend
```

Open `http://localhost:5173`, or run the containerized application:

```bash
docker compose up --build
```
