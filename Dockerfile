# 1. Planner Stage: Prepare the recipe
FROM rust:1.84-slim AS planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 2. Builder Stage: Build dependencies and binary
FROM rust:1.84-slim AS builder
WORKDIR /app
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies only (cached until Cargo.toml changes)
RUN cargo chef cook --release --recipe-path recipe.json

# Build the actual application
COPY . .
RUN cargo build --release --bin backend

# 3. Runtime Stage: The final tiny image
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
# Copy only what is strictly necessary
COPY --from=builder /app/target/release/backend /usr/local/bin/backend
COPY --from=builder /app/migrations ./migrations

EXPOSE 3003
ENTRYPOINT ["/usr/local/bin/backend"]
