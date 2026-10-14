# Database schema

PostgreSQL is the system of record. SeaORM migrations are in `backend/migration/`. The dev API applies them at startup; production uses a one-shot migration service before the API starts.

## Entity relationship diagram

```mermaid
erDiagram
    USERS ||--o{ SESSIONS : owns
    SESSIONS ||--o{ REFRESH_TOKENS : rotates
    USERS ||--o{ USER_SYSTEM_ROLES : assigned
    SYSTEM_ROLES ||--o{ USER_SYSTEM_ROLES : grants
    SYSTEM_ROLES ||--o{ SYSTEM_ROLE_PERMISSIONS : contains
    PERMISSIONS ||--o{ SYSTEM_ROLE_PERMISSIONS : names
    PERMISSIONS ||--o{ WORKSPACE_ROLE_PERMISSIONS : names
    WORKSPACES ||--o{ WORKSPACE_MEMBERS : includes
    USERS ||--o{ WORKSPACE_MEMBERS : joins
    WORKSPACES ||--o{ WORKSPACE_INVITATIONS : invites

    USERS {
        uuid id PK
        varchar email UK "case-insensitive unique index"
        text password_hash
        varchar display_name
        timestamptz email_verified_at "nullable"
        boolean is_root
        boolean must_update_credentials
        timestamptz created_at
        timestamptz updated_at
        timestamptz disabled_at "nullable"
    }

    SESSIONS {
        uuid id PK
        uuid user_id FK
        bytea token_hash UK "current refresh token digest"
        bytea access_token_hash UK "access token digest"
        bytea csrf_token_hash "CSRF digest"
        varchar device_name "nullable"
        text user_agent "nullable"
        inet ip_address "nullable"
        timestamptz created_at
        timestamptz last_seen_at
        timestamptz access_expires_at
        timestamptz expires_at "absolute session expiry"
        timestamptz revoked_at "nullable"
    }

    REFRESH_TOKENS {
        uuid id PK
        uuid session_id FK
        bytea token_hash UK "SHA-256 digest, 32 bytes"
        timestamptz created_at
        timestamptz expires_at
        timestamptz consumed_at "nullable"
        uuid replaced_by_id FK "nullable"
    }

    SYSTEM_ROLES {
        uuid id PK
        varchar key UK
        text description
    }
    PERMISSIONS {
        varchar key PK
        text description
    }
    USER_SYSTEM_ROLES {
        uuid user_id PK,FK
        uuid role_id PK,FK
        timestamptz assigned_at
    }
    SYSTEM_ROLE_PERMISSIONS {
        uuid role_id PK,FK
        varchar permission_key PK,FK
    }
    WORKSPACES {
        uuid id PK
        varchar name
        varchar slug UK
        varchar description
        uuid created_by FK
        timestamptz archived_at
    }
    WORKSPACE_MEMBERS {
        uuid workspace_id PK,FK
        uuid user_id PK,FK
        varchar role "owner/admin/member"
    }
    WORKSPACE_ROLE_PERMISSIONS {
        varchar role PK
        varchar permission_key PK,FK
    }
    WORKSPACE_INVITATIONS {
        uuid id PK
        uuid workspace_id FK
        varchar email
        varchar role
        bytea token_hash UK
        uuid invited_by FK
        timestamptz expires_at
        timestamptz accepted_at
        timestamptz revoked_at
    }
```

## Tables

### `users`

One account per normalized email address. A unique index on `LOWER(email)` prevents case variants from creating duplicates. Normalize email consistently in application code. `password_hash` stores an Argon2id password hash, never plaintext. Update `updated_at` when a user's profile changes.

`is_root` marks the first system account. `must_update_credentials` limits the seeded root account to login, logout, and updating its profile until it replaces its temporary email and password. `disabled_at` disables an account without deleting its history; authentication rejects disabled users.

### `sessions`

One row represents a device login. Multiple rows per `user_id` are allowed, so a user can sign in on multiple devices and revoke each one independently. `access_token_hash` stores the digest of the active opaque access token. Access tokens expire after 15 minutes and are checked against Postgres on each authenticated request, so logout and revocation apply immediately.

`token_hash` stores the current refresh token digest. `expires_at` is the absolute 30-day session expiry. `last_seen_at` is updated on refresh. Set `revoked_at` to invalidate a device session; deleting a user cascades to all sessions. `device_name`, `user_agent`, and `ip_address` are informational, not reliable device identity. Retain and expose them with appropriate privacy limits.

The session-bound CSRF token digest is stored in `csrf_token_hash`. Production browser cookies use `__Host-`, `Secure`, `HttpOnly` for auth tokens, and `SameSite=Lax`; state-changing requests must echo the CSRF token in `X-CSRF-Token`.

### RBAC, workspaces, and invitations

Authorization uses named roles that grant permission keys. System permissions are stored in `permissions`, attached to `system_roles`, and assigned through `user_system_roles`. The initial platform role grants workspace creation, metadata visibility/editing, and membership/invitation management. It deliberately has no `workspace_content:read` permission.

Workspace membership is independent from platform roles. Workspace roles map through `workspace_role_permissions`: owners and admins can manage workspace metadata, membership, and invitations; members can read workspace metadata and content. Content permissions are checked separately from platform administration. The platform root is never implicitly added to workspace membership.

Workspace creation requires an initial owner email. The API creates a seven-day, single-use invitation in the same transaction as the workspace and returns its raw token once; only its SHA-256 digest is stored. Existing accounts must prove their password to accept. New accounts set a password through acceptance, and the invitation link verifies the invited email. The admin UI builds a copyable URL from the token. Owner membership cannot be reduced to zero by member removal.

The versioned routes include `GET/POST /api/v1/workspaces`, `PATCH /api/v1/workspaces/{workspace_id}`, member and invitation management under `/api/v1/workspaces/{workspace_id}/...`, and `POST /api/v1/invitations/{token}/accept`. Authorization runs in application use cases for every operation; UI visibility is not used as an access control.

### `refresh_tokens`

Each row is one refresh token in a session family. A successful refresh consumes the presented token and inserts a replacement. The full hash history remains so presenting an already consumed token can be detected and the device session revoked. `replaced_by_id` records the rotation chain. Rotation does not extend the session's absolute expiry; retain history through the session expiry for replay detection.

## Authentication and authorization

The browser uses short-lived opaque access tokens plus rotating, single-use refresh tokens. This provides immediate server-side revocation and replay detection without storing bearer tokens in browser local storage. Password hashing uses Argon2id. For board features, add workspace or board membership records and check a user's role and access to the specific resource on every request; authentication alone must not grant access to every board.

## Migration operations

```sh
make migrate
make migrate-status
make migrate-down
```

Production migration commands are `make migrate-prod` and `make migrate-prod-status` with `POSTGRES_PASSWORD` set. The initial migration is `m202610130001_create_users_and_sessions`; `m202610130002_add_auth_token_rotation` adds bootstrap flags, token fields, CSRF binding, and refresh history; `m202610140001_workspaces_rbac_invitations` adds platform RBAC, workspace membership, and invitations. Rollbacks drop tables or auth state; only use them when the associated data loss is intended.
