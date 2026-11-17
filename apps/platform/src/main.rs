//! `capricorn-platform serve` runs the API, `worker` runs the jobs, and
//! `migrate` only brings the database up to date. `serve` and `worker` also
//! migrate first, so one of them is enough to start from an empty database.

use capricorn_platform::config::Config;
use capricorn_platform::state::AppState;
use capricorn_platform::{http, jobs};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let command = std::env::args().nth(1).unwrap_or_else(|| "serve".to_string());
    if !["serve", "worker", "migrate"].contains(&command.as_str()) {
        eprintln!("usage: capricorn-platform [serve|worker|migrate]");
        std::process::exit(2);
    }
    let config = Config::from_env().unwrap_or_else(|error| {
        eprintln!("capricorn-platform: {error}");
        std::process::exit(1);
    });
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|error| fail("the database could not be reached", error));
    capricorn_platform::MIGRATOR.run(&pool).await.unwrap_or_else(|error| fail("migrations failed", error));

    let state = AppState::new(pool, config);
    match command.as_str() {
        "migrate" => {}
        "worker" => {
            tracing::info!("worker started");
            tokio::select! {
                () = jobs::run_worker(state) => {}
                () = shutdown() => {}
            }
        }
        _ => {
            let addr = state.config.addr;
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .unwrap_or_else(|error| fail("the address could not be bound", error));
            tracing::info!(%addr, "platform listening");
            axum::serve(listener, http::router(state))
                .with_graceful_shutdown(shutdown())
                .await
                .unwrap_or_else(|error| fail("the server stopped", error));
        }
    }
}

fn fail(what: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("capricorn-platform: {what}: {error}");
    std::process::exit(1);
}

async fn shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("a SIGTERM handler can be installed");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
