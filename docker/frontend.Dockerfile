# syntax=docker/dockerfile:1.7
# Nginx serving the SvelteKit static build and proxying /api to the backend.
FROM node:22-bookworm-slim AS build
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend/ ./
COPY contract-fixtures/ /src/contract-fixtures/
ARG APP_BUILD=0
ENV PUBLIC_APP_BUILD=$APP_BUILD
RUN npm run build

FROM nginx:1.27-alpine
COPY docker/nginx.conf.template /etc/nginx/templates/default.conf.template
COPY docker/wordfall-headers.inc /etc/nginx/conf.d/wordfall-headers.inc
COPY --from=build /src/frontend/build /usr/share/nginx/html
ENV BACKEND_UPSTREAM=backend:8080 \
    REAL_IP_FROM=127.0.0.1 \
    HSTS=""
