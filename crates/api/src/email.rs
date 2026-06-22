//! Email delivery via SMTP (lettre, async). Used to send alert digests; the
//! caller only invokes this when SMTP is configured.

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::SmtpConfig;

/// Send a plain-text email. Uses STARTTLS, which works with common dev inboxes
/// (e.g. Mailtrap) and most providers on port 587.
pub async fn send(smtp: &SmtpConfig, subject: &str, body: String) -> anyhow::Result<()> {
    let email = Message::builder()
        .from(smtp.from.parse()?)
        .to(smtp.to.parse()?)
        .subject(subject)
        .body(body)?;

    let mut builder =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?.port(smtp.port);
    if let (Some(user), Some(pass)) = (&smtp.username, &smtp.password) {
        builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }
    let mailer = builder.build();
    mailer.send(email).await?;
    Ok(())
}
