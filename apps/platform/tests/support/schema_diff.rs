//! What the entities have that a database does not, as the SQL that adds it.
//! Shared by the schema test and the migration command.

use std::sync::{Arc, Mutex};

use sea_orm::SqlxPostgresConnector;

/// Brings the database up to the entities and returns the statements that took:
/// what a new migration needs. It changes the database, so give it a throwaway one.
pub async fn missing(pool: sqlx::PgPool) -> Result<Vec<String>, sea_orm::DbErr> {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let mut db = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    let seen = ran.clone();
    db.set_metric_callback(move |info| {
        let sql = info.statement.sql.trim();
        let upper = sql.to_ascii_uppercase();
        if !info.failed && ["CREATE ", "ALTER ", "DROP "].iter().any(|verb| upper.starts_with(verb)) {
            seen.lock().expect("statements").push(format!("{sql};"));
        }
    });
    aule_platform::entity::schema().sync(&db).await?;
    let statements = ran.lock().expect("statements").clone();
    Ok(statements)
}
