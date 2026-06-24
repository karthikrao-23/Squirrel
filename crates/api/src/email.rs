//! Email delivery via SMTP (lettre, async). Used to send alert digests; the
//! caller only invokes this when SMTP is configured.

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::SmtpConfig;

/// Build a plain-text message from `from` to `to`. Separated from transport so
/// the recipient routing (per-user, never a global address) is unit-testable.
pub fn build_message(from: &str, to: &str, subject: &str, body: String) -> anyhow::Result<Message> {
    Ok(Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .body(body)?)
}

/// Send a plain-text email to `to`. Uses STARTTLS, which works with common dev
/// inboxes (e.g. Mailtrap) and most providers on port 587. The recipient is
/// always passed explicitly by the caller (per-user) — never read from config.
pub async fn send(smtp: &SmtpConfig, to: &str, subject: &str, body: String) -> anyhow::Result<()> {
    let email = build_message(&smtp.from, to, subject, body)?;

    let mut builder =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?.port(smtp.port);
    if let (Some(user), Some(pass)) = (&smtp.username, &smtp.password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let mailer = builder.build();
    mailer.send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each user's digest is addressed to *that user's* email — never a shared
    /// global recipient. This is the highest-impact isolation guarantee for mail.
    #[test]
    fn recipient_is_per_user() {
        let from = "alerts@squirrel.local";
        let a = build_message(from, "alice@example.com", "subj", "body".into()).unwrap();
        let b = build_message(from, "bob@example.com", "subj", "body".into()).unwrap();

        let to = |m: &Message| {
            m.envelope()
                .to()
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(to(&a), vec!["alice@example.com".to_string()]);
        assert_eq!(to(&b), vec!["bob@example.com".to_string()]);
    }
}
