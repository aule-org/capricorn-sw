//! Starting the platform on any database it may meet: an empty one, the
//! server's (which ran migrations 1 to 16 before they became the baseline),
//! a backup older than that, and two processes starting at once.

use aule_platform::{BASELINE, MigrateError, migrate};
use sqlx::PgPool;

const BASELINE_SQL: &str = include_str!("../migrations/0017_baseline.sql");

/// A database as the server's was before the baseline: the same schema, with
/// migrations `1..=last` recorded the way SQLx records them.
async fn as_before(pool: &PgPool, last: i64) {
    sqlx::raw_sql(BASELINE_SQL).execute(pool).await.unwrap();
    sqlx::raw_sql(
        "create table _sqlx_migrations (
             version bigint primary key, description text not null,
             installed_on timestamptz not null default now(), success boolean not null,
             checksum bytea not null, execution_time bigint not null)",
    )
    .execute(pool)
    .await
    .unwrap();
    for version in 1..=last {
        sqlx::query(
            "insert into _sqlx_migrations (version, description, success, checksum, execution_time)
             values ($1, 'old', true, '\\x00', 1)",
        )
        .bind(version)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query("insert into families (id, name) values (gen_random_uuid(), 'Quispe')")
        .execute(pool)
        .await
        .unwrap();
}

async fn versions(pool: &PgPool) -> Vec<i64> {
    sqlx::query_scalar("select version from _sqlx_migrations order by version").fetch_all(pool).await.unwrap()
}

#[sqlx::test(migrations = false)]
async fn an_empty_database_gets_the_baseline(pool: PgPool) {
    migrate(&pool).await.unwrap();

    assert_eq!(versions(&pool).await.first(), Some(&BASELINE));
    let tables: i64 = sqlx::query_scalar(
        "select count(*) from information_schema.tables where table_name in ('sessions', 'children')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 2);
}

#[sqlx::test(migrations = false)]
async fn the_servers_database_becomes_the_baseline_without_changing(pool: PgPool) {
    as_before(&pool, 16).await;
    // A session the engine ran, as the old schema says it.
    sqlx::query(
        "insert into sessions (id, audience, room, language, agent_token_hash, flow)
         values (gen_random_uuid(), 'operator', 'room-1', 'es', 'hash-1', 'engine')",
    )
    .execute(&pool)
    .await
    .unwrap();

    migrate(&pool).await.unwrap();

    // After the baseline, the migrations that follow it ran: the engine is called the engine.
    let engine: bool = sqlx::query_scalar("select engine from sessions where room = 'room-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(engine);
    let inputs: i64 =
        sqlx::query_scalar("select count(*) from session_engine_inputs").fetch_one(&pool).await.unwrap();
    assert_eq!(inputs, 0);

    assert_eq!(versions(&pool).await.first(), Some(&BASELINE));
    assert!(versions(&pool).await.iter().all(|version| *version >= BASELINE));
    let checksum: Vec<u8> = sqlx::query_scalar("select checksum from _sqlx_migrations where version = $1")
        .bind(BASELINE)
        .fetch_one(&pool)
        .await
        .unwrap();
    let expected = aule_platform::MIGRATOR.iter().find(|m| m.version == BASELINE).unwrap().checksum.to_vec();
    assert_eq!(checksum, expected, "the migrator must see the baseline as run");
    let families: i64 = sqlx::query_scalar("select count(*) from families").fetch_one(&pool).await.unwrap();
    assert_eq!(families, 1, "nothing in the data changes");

    // Starting again changes nothing.
    let before = versions(&pool).await;
    migrate(&pool).await.unwrap();
    assert_eq!(versions(&pool).await, before);
}

#[sqlx::test(migrations = false)]
async fn a_backup_older_than_migration_16_is_refused_with_the_way_to_restore_it(pool: PgPool) {
    as_before(&pool, 7).await;

    let error = migrate(&pool).await.unwrap_err();

    assert!(matches!(error, MigrateError::PredatesBaseline(7)), "{error}");
    assert!(error.to_string().contains("pre-baseline"));
    assert_eq!(versions(&pool).await, (1..=7).collect::<Vec<_>>(), "a refused database is left as it was");
}

#[sqlx::test(migrations = false)]
async fn the_api_and_the_worker_starting_together_swap_once(pool: PgPool) {
    as_before(&pool, 16).await;

    let (api, worker) = tokio::join!(migrate(&pool), migrate(&pool));

    api.unwrap();
    worker.unwrap();
    assert_eq!(versions(&pool).await.first(), Some(&BASELINE));
    let rows: i64 = sqlx::query_scalar("select count(*) from _sqlx_migrations where version = $1")
        .bind(BASELINE)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}
