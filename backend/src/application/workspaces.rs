use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::application::{
    auth::{
        AuthenticatedUser, generate_token, hash_password, normalize_email, token_hash,
        verify_password,
    },
    error::AppError,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateWorkspaceRequest {
    #[schema(min_length = 2, max_length = 100)]
    pub name: String,
    #[schema(max_length = 500)]
    pub description: Option<String>,
    pub owner_email: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateWorkspaceRequest {
    #[schema(min_length = 2, max_length = 100)]
    pub name: String,
    #[schema(max_length = 500)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateInvitationRequest {
    pub email: String,
    #[schema(example = "member")]
    pub role: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AcceptInvitationRequest {
    #[schema(min_length = 1, max_length = 80)]
    pub display_name: String,
    #[schema(min_length = 12, max_length = 128)]
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceSummary {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub archived: bool,
    pub member_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedWorkspace {
    pub workspace: WorkspaceSummary,
    pub owner_invitation: CreatedInvitation,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedInvitation {
    pub id: Uuid,
    pub email: String,
    pub role: String,
    pub token: String,
    pub expires_in_days: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceMember {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InvitationAccepted {
    pub workspace_id: Uuid,
    pub email: String,
    pub role: String,
    pub account_created: bool,
}

pub async fn list_workspaces(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
) -> Result<Vec<WorkspaceSummary>, AppError> {
    require_permission(db, actor.id, "workspaces:read_metadata").await?;
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT w.id, w.name, w.slug, w.description, w.archived_at IS NOT NULL AS archived, (SELECT COUNT(*) FROM workspace_members m WHERE m.workspace_id = w.id) AS member_count FROM workspaces w ORDER BY w.name".to_owned(),
        ))
        .await
        .map_err(AppError::database)?;
    rows.iter().map(workspace_from_row).collect()
}

pub async fn create_workspace(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    request: CreateWorkspaceRequest,
) -> Result<CreatedWorkspace, AppError> {
    require_permission(db, actor.id, "workspaces:create").await?;
    let name = request.name.trim();
    let description = request.description.unwrap_or_default();
    if name.chars().count() < 2 || name.chars().count() > 100 || description.chars().count() > 500 {
        return Err(AppError::bad_request(
            "Workspace name or description is invalid.",
        ));
    }
    let owner_email = normalize_email(&request.owner_email)?;
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(AppError::bad_request(
            "Workspace name must include a letter or number.",
        ));
    }
    let transaction = db.begin().await.map_err(AppError::database)?;
    let inserted = transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO workspaces (name, slug, description, created_by) VALUES ($1, $2, $3, $4) RETURNING id, name, slug, description, archived_at IS NOT NULL AS archived",
            [name.to_owned().into(), slug.clone().into(), description.into(), actor.id.into()],
        ))
        .await
        .map_err(|error| {
            let message = error.to_string().to_lowercase();
            if message.contains("unique") || message.contains("duplicate") {
                AppError::conflict("A workspace with that name already exists.")
            } else {
                AppError::database(error)
            }
        })?
        .ok_or_else(AppError::internal)?;
    let workspace_id: Uuid = inserted.try_get("", "id").map_err(AppError::database)?;
    let invitation =
        insert_invitation(&transaction, workspace_id, actor.id, &owner_email, "owner").await?;
    transaction.commit().await.map_err(AppError::database)?;
    let workspace = WorkspaceSummary {
        id: workspace_id,
        name: inserted.try_get("", "name").map_err(AppError::database)?,
        slug: inserted.try_get("", "slug").map_err(AppError::database)?,
        description: inserted
            .try_get("", "description")
            .map_err(AppError::database)?,
        archived: inserted
            .try_get("", "archived")
            .map_err(AppError::database)?,
        member_count: 0,
    };
    Ok(CreatedWorkspace {
        workspace,
        owner_invitation: invitation,
    })
}

pub async fn update_workspace(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
    request: UpdateWorkspaceRequest,
) -> Result<WorkspaceSummary, AppError> {
    require_platform_or_workspace_role(db, actor.id, workspace_id, "workspaces:update_metadata")
        .await?;
    let name = request.name.trim();
    let description = request.description.unwrap_or_default();
    if name.chars().count() < 2 || name.chars().count() > 100 || description.chars().count() > 500 {
        return Err(AppError::bad_request(
            "Workspace name or description is invalid.",
        ));
    }
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(AppError::bad_request(
            "Workspace name must include a letter or number.",
        ));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE workspaces SET name = $2, slug = $3, description = $4, updated_at = NOW() WHERE id = $1 RETURNING id, name, slug, description, archived_at IS NOT NULL AS archived, (SELECT COUNT(*) FROM workspace_members m WHERE m.workspace_id = workspaces.id) AS member_count",
            [workspace_id.into(), name.to_owned().into(), slug.into(), description.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::bad_request("Workspace was not found."))?;
    workspace_from_row(&row)
}

pub async fn list_members(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
) -> Result<Vec<WorkspaceMember>, AppError> {
    require_platform_or_workspace_role(db, actor.id, workspace_id, "workspace_members:read")
        .await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT u.id AS user_id, u.email, u.display_name, m.role FROM workspace_members m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 ORDER BY m.role, u.email",
            [workspace_id.into()],
        ))
        .await
        .map_err(AppError::database)?;
    rows.iter()
        .map(|row| {
            Ok(WorkspaceMember {
                user_id: row.try_get("", "user_id").map_err(AppError::database)?,
                email: row.try_get("", "email").map_err(AppError::database)?,
                display_name: row
                    .try_get("", "display_name")
                    .map_err(AppError::database)?,
                role: row.try_get("", "role").map_err(AppError::database)?,
            })
        })
        .collect()
}

pub async fn invite_member(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
    request: CreateInvitationRequest,
) -> Result<CreatedInvitation, AppError> {
    require_platform_or_workspace_role(db, actor.id, workspace_id, "workspace_invitations:manage")
        .await?;
    let role = validate_role(&request.role)?;
    if role == "owner"
        && !is_workspace_owner_or_platform_manager(db, actor.id, workspace_id).await?
    {
        return Err(AppError::forbidden(
            "Only a workspace owner or platform manager can invite another owner.",
        ));
    }
    let email = normalize_email(&request.email)?;
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM workspaces WHERE id = $1) AS found",
            [workspace_id.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(AppError::internal)?
        .try_get::<bool>("", "found")
        .map_err(AppError::database)?;
    if !exists {
        return Err(AppError::bad_request("Workspace was not found."));
    }
    let transaction = db.begin().await.map_err(AppError::database)?;
    let invitation = insert_invitation(&transaction, workspace_id, actor.id, &email, role).await?;
    transaction.commit().await.map_err(AppError::database)?;
    Ok(invitation)
}

pub async fn revoke_invitation(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
    invitation_id: Uuid,
) -> Result<(), AppError> {
    require_platform_or_workspace_role(db, actor.id, workspace_id, "workspace_invitations:manage")
        .await?;
    let transaction = db.begin().await.map_err(AppError::database)?;
    transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM workspaces WHERE id = $1 FOR UPDATE",
            [workspace_id.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::bad_request("Workspace was not found."))?;
    let current = transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT role FROM workspace_invitations WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL FOR UPDATE",
            [invitation_id.into(), workspace_id.into()],
        ))
        .await
        .map_err(AppError::database)?;
    if let Some(current) = current {
        let role: String = current.try_get("", "role").map_err(AppError::database)?;
        if role == "owner" {
            if !is_workspace_owner_or_platform_manager(db, actor.id, workspace_id).await? {
                return Err(AppError::forbidden(
                    "Only an owner or platform manager can revoke an owner invitation.",
                ));
            }
            let remaining = transaction
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT (SELECT COUNT(*) FROM workspace_members WHERE workspace_id = $1 AND role = 'owner') + (SELECT COUNT(*) FROM workspace_invitations WHERE workspace_id = $1 AND role = 'owner' AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > NOW() AND id <> $2) AS count",
                    [workspace_id.into(), invitation_id.into()],
                ))
                .await
                .map_err(AppError::database)?
                .ok_or_else(AppError::internal)?
                .try_get::<i64>("", "count")
                .map_err(AppError::database)?;
            if remaining == 0 {
                return Err(AppError::conflict(
                    "A workspace must retain an owner or a pending owner invitation.",
                ));
            }
        }
    }
    transaction.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE workspace_invitations SET revoked_at = NOW() WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL",
        [invitation_id.into(), workspace_id.into()],
    ))
    .await
    .map_err(AppError::database)?;
    transaction.commit().await.map_err(AppError::database)
}

pub async fn remove_member(
    db: &DatabaseConnection,
    actor: &AuthenticatedUser,
    workspace_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), AppError> {
    require_platform_or_workspace_role(db, actor.id, workspace_id, "workspace_members:manage")
        .await?;
    let transaction = db.begin().await.map_err(AppError::database)?;
    transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM workspaces WHERE id = $1 FOR UPDATE",
            [workspace_id.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::bad_request("Workspace was not found."))?;
    let target = transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 FOR UPDATE",
            [workspace_id.into(), target_user_id.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::bad_request("Workspace member was not found."))?;
    let target_role: String = target.try_get("", "role").map_err(AppError::database)?;
    if target_role == "owner" {
        if !is_workspace_owner_or_platform_manager(db, actor.id, workspace_id).await? {
            transaction.rollback().await.map_err(AppError::database)?;
            return Err(AppError::forbidden(
                "Only an owner or platform manager can remove an owner.",
            ));
        }
        let owners: i64 = transaction
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*) AS count FROM workspace_members WHERE workspace_id = $1 AND role = 'owner'",
                [workspace_id.into()],
            ))
            .await
            .map_err(AppError::database)?
            .ok_or_else(AppError::internal)?
            .try_get("", "count")
            .map_err(AppError::database)?;
        if owners <= 1 {
            transaction.rollback().await.map_err(AppError::database)?;
            return Err(AppError::conflict(
                "A workspace must retain at least one owner.",
            ));
        }
    }
    transaction
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
            [workspace_id.into(), target_user_id.into()],
        ))
        .await
        .map_err(AppError::database)?;
    transaction.commit().await.map_err(AppError::database)
}

pub async fn accept_invitation(
    db: &DatabaseConnection,
    token: &str,
    request: AcceptInvitationRequest,
) -> Result<InvitationAccepted, AppError> {
    if request.password.len() > 128 {
        return Err(AppError::bad_request(
            "Password must be 128 bytes or fewer.",
        ));
    }
    if request.display_name.trim().is_empty() || request.display_name.chars().count() > 80 {
        return Err(AppError::bad_request(
            "Display name must be between 1 and 80 characters.",
        ));
    }
    let transaction = db.begin().await.map_err(AppError::database)?;
    let invitation = transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, workspace_id, email, role FROM workspace_invitations WHERE token_hash = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > NOW() FOR UPDATE",
            [token_hash(token).into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(AppError::unauthorized)?;
    let invitation_id: Uuid = invitation.try_get("", "id").map_err(AppError::database)?;
    let workspace_id: Uuid = invitation
        .try_get("", "workspace_id")
        .map_err(AppError::database)?;
    let email: String = invitation
        .try_get("", "email")
        .map_err(AppError::database)?;
    let role: String = invitation.try_get("", "role").map_err(AppError::database)?;
    let account = transaction
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, password_hash FROM users WHERE LOWER(email) = LOWER($1) AND disabled_at IS NULL",
            [email.clone().into()],
        ))
        .await
        .map_err(AppError::database)?;
    let account_created = account.is_none();
    let user_id: Uuid = if let Some(existing) = account {
        let existing_hash: String = existing
            .try_get("", "password_hash")
            .map_err(AppError::database)?;
        verify_password(existing_hash, request.password).await?;
        existing.try_get("", "id").map_err(AppError::database)?
    } else {
        let password_hash = hash_password(request.password).await?;
        transaction
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (email, password_hash, display_name, email_verified_at) VALUES ($1, $2, $3, NOW()) RETURNING id",
                [email.clone().into(), password_hash.into(), request.display_name.trim().to_owned().into()],
            ))
            .await
            .map_err(AppError::database)?
            .ok_or_else(AppError::internal)?
            .try_get("", "id")
            .map_err(AppError::database)?
    };
    transaction
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = EXCLUDED.role",
            [workspace_id.into(), user_id.into(), role.clone().into()],
        ))
        .await
        .map_err(AppError::database)?;
    transaction
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE workspace_invitations SET accepted_at = NOW() WHERE id = $1",
            [invitation_id.into()],
        ))
        .await
        .map_err(AppError::database)?;
    transaction.commit().await.map_err(AppError::database)?;
    Ok(InvitationAccepted {
        workspace_id,
        email,
        role,
        account_created,
    })
}

pub async fn has_permission(
    db: &DatabaseConnection,
    user_id: Uuid,
    permission: &str,
) -> Result<bool, AppError> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM user_system_roles ur JOIN system_role_permissions rp ON rp.role_id = ur.role_id WHERE ur.user_id = $1 AND rp.permission_key = $2) AS allowed",
            [user_id.into(), permission.to_owned().into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(AppError::internal)?;
    row.try_get("", "allowed").map_err(AppError::database)
}

async fn require_permission(
    db: &DatabaseConnection,
    user_id: Uuid,
    permission: &str,
) -> Result<(), AppError> {
    if has_permission(db, user_id, permission).await? {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "You do not have permission to perform this action.",
        ))
    }
}

async fn require_platform_or_workspace_role(
    db: &DatabaseConnection,
    user_id: Uuid,
    workspace_id: Uuid,
    permission: &str,
) -> Result<(), AppError> {
    if has_permission(db, user_id, permission).await? {
        return Ok(());
    }
    let allowed = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM workspace_members m JOIN workspace_role_permissions rp ON rp.role = m.role WHERE m.workspace_id = $1 AND m.user_id = $2 AND rp.permission_key = $3) AS allowed",
            [workspace_id.into(), user_id.into(), permission.to_owned().into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(AppError::internal)?
        .try_get::<bool>("", "allowed")
        .map_err(AppError::database)?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "You do not have permission to perform this action.",
        ))
    }
}

async fn is_workspace_owner_or_platform_manager(
    db: &DatabaseConnection,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, AppError> {
    if has_permission(db, user_id, "workspace_members:manage").await? {
        return Ok(true);
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND role = 'owner') AS allowed",
            [workspace_id.into(), user_id.into()],
        ))
        .await
        .map_err(AppError::database)?
        .ok_or_else(AppError::internal)?;
    row.try_get("", "allowed").map_err(AppError::database)
}

async fn insert_invitation<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    invited_by: Uuid,
    email: &str,
    role: &str,
) -> Result<CreatedInvitation, AppError> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE workspace_invitations SET revoked_at = NOW() WHERE workspace_id = $1 AND LOWER(email) = LOWER($2) AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at <= NOW()",
        [workspace_id.into(), email.to_owned().into()],
    ))
    .await
    .map_err(AppError::database)?;
    let token = generate_token()?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO workspace_invitations (workspace_id, email, role, token_hash, invited_by, expires_at) VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '7 days') RETURNING id",
            [workspace_id.into(), email.to_owned().into(), role.to_owned().into(), token_hash(&token).into(), invited_by.into()],
        ))
        .await
        .map_err(|error| {
            let message = error.to_string().to_lowercase();
            if message.contains("unique") || message.contains("duplicate") {
                AppError::conflict("An active invitation already exists for this email.")
            } else {
                AppError::database(error)
            }
        })?
        .ok_or_else(AppError::internal)?;
    Ok(CreatedInvitation {
        id: row.try_get("", "id").map_err(AppError::database)?,
        email: email.to_owned(),
        role: role.to_owned(),
        token,
        expires_in_days: 7,
    })
}

fn workspace_from_row(row: &sea_orm::QueryResult) -> Result<WorkspaceSummary, AppError> {
    Ok(WorkspaceSummary {
        id: row.try_get("", "id").map_err(AppError::database)?,
        name: row.try_get("", "name").map_err(AppError::database)?,
        slug: row.try_get("", "slug").map_err(AppError::database)?,
        description: row.try_get("", "description").map_err(AppError::database)?,
        archived: row.try_get("", "archived").map_err(AppError::database)?,
        member_count: row.try_get("", "member_count").unwrap_or(0),
    })
}

fn validate_role(role: &str) -> Result<&'static str, AppError> {
    match role {
        "owner" => Ok("owner"),
        "admin" => Ok("admin"),
        "member" => Ok("member"),
        _ => Err(AppError::bad_request(
            "Role must be owner, admin, or member.",
        )),
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for character in value.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}
