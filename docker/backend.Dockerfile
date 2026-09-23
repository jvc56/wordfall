# syntax=docker/dockerfile:1.7
# Backend: multi-stage Rust build -> debian:bookworm-slim (PLAN.md § Repository layout).
FROM rust:1.98-bookworm AS build
WORKDIR /src/backend
ENV SQLX_OFFLINE=true
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/backend/target \
    cargo build --release --locked --bin wordfall \
    && cp target/release/wordfall /usr/local/bin/wordfall

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/wordfall /usr/local/bin/wordfall
USER nobody
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/wordfall"]
