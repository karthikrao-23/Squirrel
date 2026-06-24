//! Rebuilds derived tax lots from stored transactions.
//!
//! This is the glue between the DB and the pure `domain::lots` reconstructor:
//! load the user's transactions, group them by (account, security), run FIFO on
//! each group, then atomically replace the stored lots. Called after every sync
//! (data changed → derived lots are stale) and exposed via `/api/lots/rebuild`.

use std::collections::BTreeMap;

use db::queries::tax_lots::NewLot;
use domain::lots::{reconstruct_fifo, LotInput};
use sqlx::PgPool;
use uuid::Uuid;

/// Reconstruct and persist all tax lots for a user. Returns the number of open
/// lots stored.
pub async fn rebuild_lots(pool: &PgPool, user_id: Uuid) -> anyhow::Result<u64> {
    let rows = db::queries::transactions::list_for_lots(pool, user_id).await?;

    // Group transactions by (account, security). BTreeMap keeps a deterministic
    // order, which keeps the resulting lot rows stable across rebuilds.
    let mut groups: BTreeMap<(Uuid, Uuid), Vec<LotInput>> = BTreeMap::new();
    for r in rows {
        groups
            .entry((r.account_id, r.security_id))
            .or_default()
            .push(LotInput {
                source_transaction_id: r.id,
                date: r.date,
                transaction_type: r.transaction_type,
                quantity: r.quantity,
                price: r.price,
                amount: r.amount,
                fees: r.fees,
            });
    }

    let mut new_lots: Vec<NewLot> = Vec::new();
    for ((account_id, security_id), inputs) in groups {
        for lot in reconstruct_fifo(&inputs) {
            new_lots.push(NewLot {
                account_id,
                security_id,
                open_date: lot.open_date,
                original_quantity: lot.original_quantity,
                remaining_quantity: lot.remaining_quantity,
                cost_basis_per_share: lot.cost_basis_per_share,
                source_transaction_id: Some(lot.source_transaction_id),
            });
        }
    }

    let count = db::queries::tax_lots::replace_for_user(pool, user_id, &new_lots).await?;
    tracing::info!(lots = count, user = %user_id, "tax lots rebuilt");
    Ok(count)
}
