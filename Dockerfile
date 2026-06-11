# ============================================================
# Stage 1: Install cargo-chef (dependency analysis tool)
# ============================================================
FROM rust:1.95-alpine AS chef

RUN apk add --no-cache musl-dev pkgconfig
RUN cargo install cargo-chef --locked

WORKDIR /geello

# ============================================================
# Stage 2: Generate recipe.json (dependency recipe file)
# ============================================================
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# ============================================================
# Stage 3: Build dependencies using recipe.json, then compile
# ============================================================
FROM chef AS builder

COPY --from=planner /geello/recipe.json recipe.json

# Build dependencies first (cached by Docker until Cargo.toml/lock changes)
RUN cargo chef cook --release \
    --target x86_64-unknown-linux-musl \
    --recipe-path recipe.json \
    --features full

# Copy full source and build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets

RUN cargo build --release \
    --target x86_64-unknown-linux-musl \
    --features full

# ============================================================
# Stage 4: Minimal runtime image
# ============================================================
FROM alpine:3.22

WORKDIR /geello

COPY --from=builder \
    /geello/target/x86_64-unknown-linux-musl/release/geello .

COPY data ./data

EXPOSE 8000

ENTRYPOINT ["./geello"]
