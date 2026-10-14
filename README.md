# Treefime

Treefime is a Trello-like project management starter with a Rust API, a Next.js web app, and PostgreSQL. The repository has two application folders: `backend/` and `frontend/`.

## Development

Requirements: Docker, Docker Compose, and Make.

```sh
make dev
```

Open the frontend at [http://localhost:3000](http://localhost:3000). The API listens on port `8080`, and PostgreSQL is exposed on port `5432` for local development. The local database is `treefime` with username and password `treefime-dev-only`.

Start the backend and its database with `make dev-backend`. Start the frontend independently with `make dev-frontend`; API requests need the backend to be running too. Stop dev services with `make down-dev`. Data remains in named volumes.

## Production profile

Set a strong database password in the shell, then start the `prod` Compose profile:

```sh
export POSTGRES_PASSWORD='use-a-long-random-value'
make prod
```

The production profile runs a one-shot migration service before the API, enables secure cookies, and keeps Postgres off the host network. Use `make logs-prod` to view service logs, including the one-time bootstrap root credentials on a fresh database. `make down-prod` stops the stack without deleting database data. This Compose setup targets a single host; provide TLS termination and production secret management in the deployment environment.

## API

All API routes use the versioned prefix `/api/v1/{path}`. The health route is `GET /api/v1/health` and returns `{"status":"ok"}`. Scalar documentation is served at [http://localhost:8080/api/v1/docs](http://localhost:8080/api/v1/docs); the OpenAPI document is at `/api/v1/openapi.json`.

The frontend proxies `/api/v1/*` to the API so browser cookies stay same-origin.

## Authentication

The first API startup against an empty database seeds a root account with a random email and password, then prints both to the API logs. With `make dev`, the credentials appear in the foreground; in production, use `make logs-prod`. The credentials are printed only when the account is first created. Sign in through the web app and replace both before using other authenticated API actions.

Passwords use Argon2id. Browser sessions use 15-minute opaque access tokens and single-use rotating refresh tokens with a 30-day absolute session expiry. Only SHA-256 token digests are stored. Reuse of a consumed refresh token revokes that device session. The API checks active access tokens against Postgres, so logout and revocation take effect immediately. Cookies are `HttpOnly`; state-changing requests require a session-bound CSRF token.

The current auth routes are:

- `GET /api/v1/auth/csrf`
- `POST /api/v1/auth/login`
- `POST /api/v1/auth/refresh`
- `POST /api/v1/auth/logout`
- `GET /api/v1/auth/me`
- `PATCH /api/v1/auth/profile`

There is no open public registration route. New accounts can be created by accepting a valid workspace invitation. The bootstrap root account is restricted to login, logout, and replacing its temporary email and password until that profile update succeeds.

## Workspace administration

The platform root can open `/admin` to create a workspace, invite its initial owner, inspect workspace metadata and membership, and issue additional invitations. The root remains outside every workspace and cannot access workspace content. Invitation links are single-use, expire after seven days, and are displayed only when created; copy and deliver the link to the invited person. Existing accounts must enter their current password to accept, while new users set a password during acceptance.

System and workspace permissions are separate. System role bundles grant keys such as `workspaces:create`, `workspaces:read_metadata`, and `workspace_invitations:manage`; workspace role bundles grant membership-scoped keys. Owners and admins manage metadata and membership. Members receive workspace content access, which will be enforced by content routes as those features are added.

## Backend structure and migrations

The Rust API uses ports and adapters, with domain types, application use cases, ports, and Axum HTTP adapters separated under `backend/src/`. Schema migrations are in `backend/migration/`; the diagram and table notes are in [docs/DATABASE.md](docs/DATABASE.md).

Development applies pending migrations when the API starts. Production runs migrations in a one-shot Compose service before starting the API. Run explicit local migration commands with Make:

```sh
make migrate
make migrate-status
make migrate-down
```

For production, use `make migrate-prod` and `make migrate-prod-status` with `POSTGRES_PASSWORD` set.

## Folders

```text
backend/          Rust API, auth module, and SeaORM migrations
frontend/         Next.js app with shadcn/ui components
docs/DATABASE.md  Schema diagram and notes
docker-compose.yml
Makefile
```
