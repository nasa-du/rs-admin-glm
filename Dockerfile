# Multi-stage production image (issue #21, spec §9.3):
# frontend build → backend build → slim runtime serving both same-origin.
# Build from the repo root:  docker build -f Dockerfile -t rs-admin .

# ---- 1. frontend build ----------------------------------------------------
FROM node:22-alpine AS frontend-builder
WORKDIR /build
# Lockfile-first copy keeps npm ci layer-cached across source changes.
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
# tsc -b + vite build → /build/dist (routeTree.gen.ts is committed)
RUN npm run build

# ---- 2. backend build -----------------------------------------------------
FROM rust:1.94-slim-bookworm AS backend-builder
WORKDIR /build
# rust-toolchain.toml is deliberately NOT copied: the image pins 1.94 itself
# and the file's component list (rustfmt/clippy) would trigger extra rustup
# downloads that a release build never uses.
# Dependencies first (dummy sources), then the real crate — so source edits
# don't rebuild sqlx/tokio/axum. sqlx queries are runtime-checked (no
# query! macros): no DATABASE_URL needed at build time.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src \
    && printf 'fn main() {}' > src/main.rs \
    && printf '' > src/lib.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/rs_admin_backend* target/release/rs-admin-backend
# sqlx::migrate! embeds ./migrations at compile time — copy before building.
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN touch src/main.rs src/lib.rs && cargo build --release

# ---- 3. runtime -----------------------------------------------------------
# bookworm-slim matches the builder's glibc (rust:1.94-slim-bookworm).
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system app && useradd --system --gid app app
WORKDIR /app
COPY --from=backend-builder /build/target/release/rs-admin-backend /usr/local/bin/rs-admin-backend
COPY --from=frontend-builder /build/dist ./frontend-dist
# Same-origin defaults (spec §9.3): axum serves the SPA beside /api on one
# port; JSON logs and no Swagger in production (spec §9.2).
ENV APP_ENV=production \
    HOST=0.0.0.0 \
    PORT=3000 \
    FRONTEND_DIST=/app/frontend-dist \
    RUST_LOG=info,sqlx=warn
USER app
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/rs-admin-backend"]
