use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE TABLE users (
                    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                    email VARCHAR(320) NOT NULL,
                    password_hash TEXT NOT NULL,
                    display_name VARCHAR(80) NOT NULL,
                    email_verified_at TIMESTAMPTZ,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    disabled_at TIMESTAMPTZ
                )
                "#,
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX users_email_lowercase_uq ON users (LOWER(email))",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE TABLE sessions (
                    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    token_hash BYTEA NOT NULL UNIQUE,
                    device_name VARCHAR(100),
                    user_agent TEXT,
                    ip_address INET,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMPTZ NOT NULL,
                    revoked_at TIMESTAMPTZ,
                    CONSTRAINT sessions_token_hash_length_ck CHECK (octet_length(token_hash) = 32),
                    CONSTRAINT sessions_expiration_ck CHECK (expires_at > created_at)
                )
                "#,
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX sessions_user_created_idx ON sessions (user_id, created_at DESC)",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX sessions_active_expiration_idx ON sessions (expires_at) WHERE revoked_at IS NULL",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE sessions")
            .await?;
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE users")
            .await?;

        Ok(())
    }
}
