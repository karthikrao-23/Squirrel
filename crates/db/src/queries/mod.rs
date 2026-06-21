//! Query layer: one module per entity, each owning the SQL for that table.
//! We use SQLx's *runtime* query API (`query_as` / `query_scalar` with bound
//! params) rather than the compile-time `query!` macro, so the workspace builds
//! and CI runs without a live database.

pub mod accounts;
pub mod holdings;
pub mod plaid_items;
pub mod securities;
pub mod tax_lots;
pub mod transactions;
pub mod users;
