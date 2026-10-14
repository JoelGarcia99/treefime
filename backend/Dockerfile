FROM rust:1.92-slim AS development
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config && rm -rf /var/lib/apt/lists/*
EXPOSE 8080
CMD ["cargo", "run", "--bin", "treefime-api"]

FROM rust:1.92-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY migration ./migration
COPY src ./src
RUN cargo build --release \
    --package treefime-api --bin treefime-api \
    --package treefime-migration --bin treefime-migrate

FROM debian:bookworm-slim AS production
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/treefime-api /usr/local/bin/treefime-api
COPY --from=builder /app/target/release/treefime-migrate /usr/local/bin/treefime-migrate
EXPOSE 8080
CMD ["treefime-api"]
