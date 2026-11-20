//! The tables as SeaORM entities. Each one lists the columns the code reads and
//! writes. The entities are where a table or a column is added: the migration
//! that creates it is generated from them (`cargo run --example migration`), and
//! a test fails while an entity has something the migrations do not create.

pub mod child;
pub mod family;
pub mod guardian;
pub mod guardian_child;

/// Every entity, in one schema: what the migrations must create.
pub fn schema() -> sea_orm::SchemaBuilder {
    sea_orm::Schema::new(sea_orm::DbBackend::Postgres)
        .builder()
        .register(family::Entity)
        .register(guardian::Entity)
        .register(child::Entity)
        .register(guardian_child::Entity)
}
