//! The entities and the migrations describe the same tables.

#[path = "support/schema_diff.rs"]
mod schema_diff;

use sqlx::PgPool;

#[sqlx::test(migrator = "aule_platform::MIGRATOR")]
async fn the_migrations_create_what_the_entities_say(pool: PgPool) {
    let missing = schema_diff::missing(pool).await.expect("the schema could be read");
    assert!(
        missing.is_empty(),
        "an entity has what no migration creates; `cargo run --example migration -- <name>` writes it:\n{}",
        missing.join("\n")
    );
}
