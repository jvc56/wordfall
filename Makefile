# Test targets (PLAN.md § Running the tests). Each runs with no arguments.
SHELL := /bin/bash
.ONESHELL:
.SHELLFLAGS := -e -o pipefail -c

# Node 20+ is needed; a user-local install under ~/.local/n is used when present.
NODE_BIN ?= $(wildcard $(HOME)/.local/n/bin)
ifneq ($(NODE_BIN),)
export PATH := $(NODE_BIN):$(PATH)
endif

# Playwright has no build for Ubuntu 20.04; its Ubuntu 22.04 Chromium runs there.
ifneq ($(shell grep -s '^VERSION_ID="20.04"' /etc/os-release),)
export PLAYWRIGHT_HOST_PLATFORM_OVERRIDE ?= ubuntu22.04-x64
endif

# Queries are compile-checked against the committed .sqlx data, so no
# database is needed to build.
export SQLX_OFFLINE := true

.PHONY: infra-validate test test-unit test-integration test-e2e test-scale test-parity \
        frontend-deps sqlx-db sqlx-prepare

test: test-unit test-integration test-e2e

frontend-deps:
	cd frontend && { [ -d node_modules ] || npm ci; }

## cargo test --lib, npm run check, Vitest. No services.
test-unit: frontend-deps
	cd backend && cargo test --lib
	cd ../frontend && npm run check && npx vitest run --passWithNoTests

## cargo test --test '*' against a throwaway Postgres it starts itself.
test-integration:
	cid=$$(docker run -d --rm -e POSTGRES_PASSWORD=wordfall -p 127.0.0.1::5432 \
	  --tmpfs /var/lib/postgresql/data postgres:16 \
	  -c fsync=off -c synchronous_commit=off -c full_page_writes=off -c max_connections=400)
	trap 'docker rm -f $$cid >/dev/null' EXIT
	port=$$(docker port $$cid 5432/tcp | head -1 | sed 's/.*://')
	for i in $$(seq 1 120); do
	  docker exec $$cid pg_isready -h 127.0.0.1 -U postgres >/dev/null 2>&1 && break
	  sleep 0.5
	done
	cd backend
	DATABASE_URL=postgres://postgres:wordfall@127.0.0.1:$$port/postgres cargo test --test '*'

## stack.up, stack.seed, Playwright, stack.down.
test-e2e: frontend-deps
	cd e2e && { [ -d node_modules ] || npm ci; } && npx playwright install chromium >/dev/null
	npx playwright test

## The 300,000-question budgets.
test-scale:
	python3 scripts/scale.py

## Zyzzyva comparison; skipped with a notice unless the licensed files are present.
test-parity:
	python3 scripts/parity.py

## Development helpers for compile-checked queries: a Postgres on :55432
## holding the schema, and regeneration of backend/.sqlx.
sqlx-db:
	docker rm -f wordfall-sqlx >/dev/null 2>&1 || true
	docker run -d --name wordfall-sqlx -e POSTGRES_USER=wordfall -e POSTGRES_PASSWORD=wordfall \
	  -e POSTGRES_DB=wordfall -p 127.0.0.1:55432:5432 postgres:16 >/dev/null
	for i in $$(seq 1 120); do
	  docker exec wordfall-sqlx pg_isready -h 127.0.0.1 -U wordfall >/dev/null 2>&1 && break
	  sleep 0.5
	done
	docker exec -i wordfall-sqlx psql -v ON_ERROR_STOP=1 -q -U wordfall -d wordfall \
	  < backend/migrations/0001_initial.sql

sqlx-prepare:
	cd backend && SQLX_OFFLINE=false DATABASE_URL=postgres://wordfall:wordfall@127.0.0.1:55432/wordfall \
	  cargo sqlx prepare -- --all-targets

## terraform validate for infra/, in a container.
TERRAFORM := docker run --rm -u $$(id -u):$$(id -g) -v $(CURDIR)/infra:/infra -w /infra hashicorp/terraform:1.9
infra-validate:
	$(TERRAFORM) init -backend=false -input=false >/dev/null
	$(TERRAFORM) validate
	$(TERRAFORM) fmt -check -recursive
