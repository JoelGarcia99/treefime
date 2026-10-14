pub use sea_orm_migration::prelude::*;

mod m202610130001_create_users_and_sessions;
mod m202610130002_add_auth_token_rotation;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m202610130001_create_users_and_sessions::Migration),
            Box::new(m202610130002_add_auth_token_rotation::Migration),
        ]
    }
}
