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
# The nightly dump runs on this image too (PLAN.md § Backups): python3 and
# boto3 for scripts/backup.py, and a Postgres 16 client from PGDG, since
# pg_dump must be at least the server's version and bookworm ships 15.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gnupg python3 python3-boto3 \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -fsSo /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    && echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" \
       > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends postgresql-client-16 \
    && apt-get purge -y gnupg && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/backup.py scripts/restore.py /opt/wordfall/scripts/
COPY --from=build /usr/local/bin/wordfall /usr/local/bin/wordfall
USER nobody
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/wordfall"]
