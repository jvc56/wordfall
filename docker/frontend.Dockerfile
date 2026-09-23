# syntax=docker/dockerfile:1.7
# Nginx serving the SvelteKit static build and proxying /api to the backend.
FROM node:22-bookworm-slim AS build
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend/ ./
COPY contract-fixtures/ /src/contract-fixtures/
# The monotonic build number and git hash (PLAN.md § API → POST /api/sync).
ARG APP_BUILD=0
ARG APP_COMMIT=dev
ENV WORDFALL_BUILD=$APP_BUILD WORDFALL_COMMIT=$APP_COMMIT
RUN npm run build

FROM nginx:1.27-alpine
COPY docker/nginx.conf.template /etc/nginx/templates/default.conf.template
COPY docker/wordfall-headers.inc /etc/nginx/conf.d/wordfall-headers.inc
COPY --from=build /src/frontend/build /usr/share/nginx/html
ENV BACKEND_UPSTREAM=backend:8080 \
    REAL_IP_FROM=127.0.0.1 \
    HSTS=""
