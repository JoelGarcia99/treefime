mod adapters;
mod application;
mod domain;
mod ports;

use adapters::inbound::http::router;
use application::auth::AuthState;
use sea_orm::{ConnectOptions, Database};
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://treefime:treefime@localhost:5432/treefime".to_owned());
    let mut database_options = ConnectOptions::new(database_url);
    database_options.max_connections(5);
    let database = Database::connect(database_options)
        .await
        .expect("failed to connect to PostgreSQL");
    let auto_migrate = std::env::var("AUTO_MIGRATE")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(true);
    if auto_migrate {
        treefime_migration::Migrator::up(&database, None)
            .await
            .expect("failed to apply database migrations");
    }

    let auth = AuthState {
        database,
        secure_cookies: std::env::var("COOKIE_SECURE")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
    };
    auth.seed_root().await.expect("failed to seed root account");

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_owned());
    let address = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&address)
        .await
        .expect("failed to bind API listener");

    println!("Treefime API listening on {address}");
    axum::serve(listener, router(auth))
        .await
        .expect("API server failed");
}
