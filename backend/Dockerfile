FROM rust:1.92-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migration ./migration
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/treefime-api /usr/local/bin/treefime-api
EXPOSE 8080
CMD ["treefime-api"]
