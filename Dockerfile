# Development Dockerfile
FROM rust:latest as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml ./

# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies only (this will be cached)
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src
COPY migrations ./migrations

# Build application (touch to ensure rebuild)
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/backend .
COPY --from=builder /app/migrations ./migrations

# Expose port
EXPOSE 3003

# Run the binary
CMD ["./backend"]
