.PHONY: dev dev-backend dev-frontend backend frontend prod down-dev down-prod logs-dev logs-prod migrate migrate-status migrate-down migrate-prod migrate-prod-status

# Start the full development stack in the foreground with live source mounts.
dev:
	docker compose --profile dev up --build postgres-dev backend-dev frontend-dev

# Start only the Rust API and its PostgreSQL dependency.
dev-backend:
	docker compose --profile dev up --build postgres-dev backend-dev

# Start only the Next.js development server.
dev-frontend:
	docker compose --profile dev up --build frontend-dev

backend: dev-backend

frontend: dev-frontend

# Build and start the production profile in the background.
prod:
	@test -n "$${POSTGRES_PASSWORD:-}" || (echo "Set POSTGRES_PASSWORD in the environment before starting prod." && exit 1)
	docker compose --profile prod up --build -d postgres-prod migrate-prod backend-prod frontend-prod

down-dev:
	docker compose --profile dev down

down-prod:
	docker compose --profile prod down

logs-dev:
	docker compose --profile dev logs -f

logs-prod:
	docker compose --profile prod logs -f

migrate:
	docker compose --profile dev run --rm migrate-dev up

migrate-status:
	docker compose --profile dev run --rm migrate-dev status

migrate-down:
	docker compose --profile dev run --rm migrate-dev down

migrate-prod:
	docker compose --profile prod run --rm migrate-prod up

migrate-prod-status:
	docker compose --profile prod run --rm migrate-prod status
