# Treefime

Treefime is a Trello-like project management starter with a Rust API, a Next.js web app, and PostgreSQL. The repository has two application folders: `backend/` and `frontend/`.

## Run the stack

Requirements: Docker and Docker Compose.

```sh
docker compose up --build
```

Open the frontend at [http://localhost:3000](http://localhost:3000). The API listens on port `8080`, and PostgreSQL is exposed on port `5432` for local development. Stop the services with `docker compose down`; add `-v` only when you also want to remove the persisted database volume.

The local development database is `treefime` with username and password `treefime`. Change these values before using the stack outside local development. The API applies pending SeaORM migrations when it starts.

## API

All API routes use the versioned prefix `/api/v1/{path}`. The first route is:

```text
GET http://localhost:8080/api/v1/health
```

It returns `200 OK` with `{"status":"ok"}`. The backend requires a reachable PostgreSQL database at startup and reads its connection string from `DATABASE_URL`.

## Backend structure

The Rust service uses ports and adapters:

- `backend/src/domain/` contains core domain types.
- `backend/src/application/` contains use cases.
- `backend/src/ports/` defines application-facing interfaces.
- `backend/src/adapters/inbound/http.rs` exposes the HTTP API through Axum.

Add new HTTP endpoints under `/api/v1/` and keep transport concerns in the HTTP adapter. PostgreSQL is provided by the Compose stack. Schema migrations live in `backend/migration/`; the database diagram and table notes are in [docs/DATABASE.md](docs/DATABASE.md).

To run the migration CLI against an existing database, set `DATABASE_URL` and run `cargo run --manifest-path backend/migration/Cargo.toml -- status` or replace `status` with `up`.

## Folders

```text
backend/    Rust API and PostgreSQL connection
frontend/   Next.js application
docker-compose.yml
```
