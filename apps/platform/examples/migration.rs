//! Writes the migration the entities need, so a table or a column is added by
//! changing its entity, not by writing SQL.
//!
//! ```bash
//! DATABASE_URL=postgres://postgres@localhost:5432/postgres cargo run --example migration -- add_something
//! ```
//!
//! It makes a throwaway database, runs the migrations there, lets SeaORM add
//! what the entities have and the migrations do not, and writes those
//! statements as the next file in `migrations/`. Review it before merging: this
//! only adds tables, columns, unique keys and foreign keys. A changed type, a
//! rename, a removal or a check constraint is still written by hand.

#[path = "../tests/support/schema_diff.rs"]
mod schema_diff;

use std::path::Path;

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = std::env::args().nth(1).ok_or("usage: cargo run --example migration -- <name>")?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err("the name is lowercase letters, digits and underscores".into());
    }
    let url = std::env::var("DATABASE_URL")?;
    let admin = PgPoolOptions::new().max_connections(1).connect(&url).await?;
    // Only digits are added to the name, so it is safe to put in the statement.
    let scratch = format!("aule_migration_{}", std::process::id());
    sqlx::query(sqlx::AssertSqlSafe(format!("create database {scratch}"))).execute(&admin).await?;

    let result = async {
        let base = url.rsplit_once('/').map_or(url.as_str(), |(base, _)| base);
        let pool = PgPoolOptions::new().max_connections(2).connect(&format!("{base}/{scratch}")).await?;
        aule_platform::MIGRATOR.run(&pool).await?;
        let missing = schema_diff::missing(pool.clone()).await?;
        pool.close().await;
        Ok::<_, Box<dyn std::error::Error>>(missing)
    }
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("drop database if exists {scratch} with (force)")))
        .execute(&admin)
        .await?;
    let missing = result?;

    if missing.is_empty() {
        println!("The migrations already create everything the entities have.");
        return Ok(());
    }
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let next = std::fs::read_dir(&folder)?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.split('_').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let path = folder.join(format!("{next:04}_{name}.sql"));
    let body = format!(
        "-- Generated from the entities by `cargo run --example migration`.\n{}\n",
        missing.join("\n")
    );
    std::fs::write(&path, body)?;
    println!("Wrote {}", path.display());
    Ok(())
}
