use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in [
            r#"CREATE TABLE permissions (
                key VARCHAR(100) PRIMARY KEY,
                description TEXT NOT NULL
            )"#,
            r#"CREATE TABLE system_roles (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                key VARCHAR(80) NOT NULL UNIQUE,
                description TEXT NOT NULL
            )"#,
            r#"CREATE TABLE system_role_permissions (
                role_id UUID NOT NULL REFERENCES system_roles(id) ON DELETE CASCADE,
                permission_key VARCHAR(100) NOT NULL REFERENCES permissions(key) ON DELETE CASCADE,
                PRIMARY KEY (role_id, permission_key)
            )"#,
            r#"CREATE TABLE user_system_roles (
                user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role_id UUID NOT NULL REFERENCES system_roles(id) ON DELETE CASCADE,
                assigned_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, role_id)
            )"#,
            r#"CREATE TABLE workspaces (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name VARCHAR(100) NOT NULL,
                slug VARCHAR(100) NOT NULL UNIQUE,
                description VARCHAR(500) NOT NULL DEFAULT '',
                archived_at TIMESTAMPTZ,
                created_by UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"CREATE TABLE workspace_members (
                workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role VARCHAR(20) NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
                joined_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (workspace_id, user_id)
            )"#,
            r#"CREATE TABLE workspace_role_permissions (
                role VARCHAR(20) NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
                permission_key VARCHAR(100) NOT NULL REFERENCES permissions(key) ON DELETE CASCADE,
                PRIMARY KEY (role, permission_key)
            )"#,
            r#"CREATE INDEX workspace_members_user_idx ON workspace_members (user_id, workspace_id)"#,
            r#"CREATE TABLE workspace_invitations (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                email VARCHAR(320) NOT NULL,
                role VARCHAR(20) NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
                token_hash BYTEA NOT NULL UNIQUE,
                invited_by UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                expires_at TIMESTAMPTZ NOT NULL,
                accepted_at TIMESTAMPTZ,
                revoked_at TIMESTAMPTZ,
                CONSTRAINT workspace_invitations_hash_length_ck CHECK (octet_length(token_hash) = 32),
                CONSTRAINT workspace_invitations_expiration_ck CHECK (expires_at > created_at)
            )"#,
            r#"CREATE INDEX workspace_invitations_workspace_idx ON workspace_invitations (workspace_id, created_at DESC)"#,
            r#"CREATE UNIQUE INDEX workspace_invitations_active_email_uq ON workspace_invitations (workspace_id, LOWER(email)) WHERE accepted_at IS NULL AND revoked_at IS NULL"#,
            r#"INSERT INTO permissions (key, description) VALUES
                ('workspaces:create', 'Create workspaces and invite their initial owner'),
                ('workspaces:read_metadata', 'Read workspace names, status, and membership metadata'),
                ('workspaces:update_metadata', 'Update workspace metadata'),
                ('workspace_content:read', 'Read boards, cards, and other workspace content'),
                ('workspace_members:read', 'Read workspace membership metadata'),
                ('workspace_members:manage', 'Manage workspace memberships and roles'),
                ('workspace_invitations:manage', 'Create and revoke workspace invitations')"#,
            r#"INSERT INTO workspace_role_permissions (role, permission_key) VALUES
                ('owner', 'workspaces:read_metadata'), ('owner', 'workspaces:update_metadata'), ('owner', 'workspace_members:read'), ('owner', 'workspace_members:manage'), ('owner', 'workspace_invitations:manage'),
                ('owner', 'workspace_content:read'),
                ('admin', 'workspaces:read_metadata'), ('admin', 'workspaces:update_metadata'), ('admin', 'workspace_members:read'), ('admin', 'workspace_members:manage'), ('admin', 'workspace_invitations:manage'), ('admin', 'workspace_content:read'),
                ('member', 'workspaces:read_metadata'), ('member', 'workspace_content:read')"#,
            r#"INSERT INTO system_roles (key, description) VALUES
                ('platform_workspace_manager', 'Manage workspace setup and membership without workspace content access')"#,
            r#"INSERT INTO system_role_permissions (role_id, permission_key)
                SELECT r.id, p.key FROM system_roles r CROSS JOIN permissions p
                WHERE r.key = 'platform_workspace_manager' AND p.key IN (
                    'workspaces:create', 'workspaces:read_metadata', 'workspaces:update_metadata',
                    'workspace_members:read', 'workspace_members:manage', 'workspace_invitations:manage'
                )"#,
            r#"INSERT INTO user_system_roles (user_id, role_id)
                SELECT u.id, r.id FROM users u CROSS JOIN system_roles r
                WHERE u.is_root = TRUE AND r.key = 'platform_workspace_manager'
                ON CONFLICT DO NOTHING"#,
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
            "DROP TABLE workspace_invitations",
            "DROP TABLE workspace_members",
            "DROP TABLE workspace_role_permissions",
            "DROP TABLE workspaces",
            "DROP TABLE user_system_roles",
            "DROP TABLE system_role_permissions",
            "DROP TABLE system_roles",
            "DROP TABLE permissions",
        ] {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        Ok(())
    }
}
