# syntax=docker/dockerfile:1.7

FROM rust:1.86-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runner

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home appuser

WORKDIR /app

COPY --from=builder /app/target/release/backend-rust-2 /app/backend-rust-2

USER appuser

ENV APP_ENV=production
ENV PORT=8000

EXPOSE 8000

CMD ["/app/backend-rust-2"]
