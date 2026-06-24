//! Typed row models mapping to the database schema. These derive `FromRow` so
//! SQLx can map query results directly, and `Serialize` so handlers can return
//! them as JSON.

use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Clone, Serialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub filing_status: String,
    pub taxable_income: Decimal,
    // Argon2id PHC string. Nullable: legacy/seed rows may have none, in which
    // case authentication is impossible (never treated as a match). Never
    // serialized into a response.
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Hand-rolled so a stray `tracing::debug!(?user)` can't leak the password hash.
// We redact it explicitly rather than deriving `Debug`.
impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("filing_status", &self.filing_status)
            .field("taxable_income", &self.taxable_income)
            .field(
                "password_hash",
                &self.password_hash.as_ref().map(|_| "<redacted>"),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// A login session. We persist only the SHA-256 of the opaque token (`token_hash`);
/// the raw token lives solely in the user's cookie.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    #[serde(skip_serializing)]
    pub token_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PlaidItem {
    pub id: Uuid,
    pub user_id: Uuid,
    pub plaid_item_id: String,
    // Encrypted token bytes (Postgres BYTEA); never serialized into responses.
    #[serde(skip_serializing)]
    pub access_token_encrypted: Vec<u8>,
    pub institution_id: Option<String>,
    pub institution_name: Option<String>,
    pub transactions_cursor: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub user_id: Uuid,
    pub plaid_item_id: Uuid,
    pub plaid_account_id: String,
    pub name: String,
    pub official_name: Option<String>,
    pub r#type: Option<String>,
    pub subtype: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Security {
    pub id: Uuid,
    pub plaid_security_id: String,
    pub ticker: Option<String>,
    pub name: Option<String>,
    pub cusip: Option<String>,
    pub r#type: Option<String>,
    pub close_price: Option<Decimal>,
    pub close_price_as_of: Option<NaiveDate>,
    pub currency: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Holding {
    pub id: Uuid,
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub quantity: Decimal,
    pub institution_price: Option<Decimal>,
    pub institution_price_as_of: Option<NaiveDate>,
    pub institution_value: Option<Decimal>,
    pub cost_basis: Option<Decimal>,
    pub currency: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Transaction {
    pub id: Uuid,
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub security_id: Option<Uuid>,
    pub plaid_investment_transaction_id: String,
    pub r#type: Option<String>,
    pub subtype: Option<String>,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
    pub date: NaiveDate,
    pub name: Option<String>,
    pub currency: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TaxLot {
    pub id: Uuid,
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub open_date: NaiveDate,
    pub quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub cost_basis_per_share: Decimal,
    pub status: String,
    pub source_transaction_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Alert {
    pub id: Uuid,
    pub user_id: Uuid,
    pub r#type: String,
    pub security_id: Option<Uuid>,
    pub title: String,
    pub message: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub emailed_at: Option<DateTime<Utc>>,
}
