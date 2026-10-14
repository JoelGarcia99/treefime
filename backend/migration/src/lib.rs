pub use sea_orm_migration::prelude::*;

mod m202610130001_create_users_and_sessions;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m202610130001_create_users_and_sessions::Migration)]
    }
}
