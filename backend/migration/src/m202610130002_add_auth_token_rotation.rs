use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in [
            "ALTER TABLE users ADD COLUMN is_root BOOLEAN NOT NULL DEFAULT FALSE",
            "ALTER TABLE users ADD COLUMN must_update_credentials BOOLEAN NOT NULL DEFAULT FALSE",
            "ALTER TABLE sessions ADD COLUMN access_token_hash BYTEA UNIQUE",
            "ALTER TABLE sessions ADD COLUMN access_expires_at TIMESTAMPTZ",
            "ALTER TABLE sessions ADD COLUMN csrf_token_hash BYTEA",
            "ALTER TABLE sessions ADD CONSTRAINT sessions_access_hash_length_ck CHECK (access_token_hash IS NULL OR octet_length(access_token_hash) = 32)",
            "ALTER TABLE sessions ADD CONSTRAINT sessions_csrf_hash_length_ck CHECK (csrf_token_hash IS NULL OR octet_length(csrf_token_hash) = 32)",
            r#"
                CREATE TABLE refresh_tokens (
                    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    token_hash BYTEA NOT NULL UNIQUE,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMPTZ NOT NULL,
                    consumed_at TIMESTAMPTZ,
                    replaced_by_id UUID REFERENCES refresh_tokens(id),
                    CONSTRAINT refresh_tokens_hash_length_ck CHECK (octet_length(token_hash) = 32),
                    CONSTRAINT refresh_tokens_expiration_ck CHECK (expires_at > created_at)
                )
            "#,
            "CREATE INDEX refresh_tokens_session_idx ON refresh_tokens (session_id, created_at DESC)",
            "CREATE INDEX refresh_tokens_expiration_idx ON refresh_tokens (expires_at) WHERE consumed_at IS NULL",
        ] {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in [
            "DROP TABLE refresh_tokens",
            "ALTER TABLE sessions DROP COLUMN access_expires_at",
            "ALTER TABLE sessions DROP COLUMN access_token_hash",
            "ALTER TABLE sessions DROP COLUMN csrf_token_hash",
            "ALTER TABLE users DROP COLUMN must_update_credentials",
            "ALTER TABLE users DROP COLUMN is_root",
        ] {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }

        Ok(())
    }
}
