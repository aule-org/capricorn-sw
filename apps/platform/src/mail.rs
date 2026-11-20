//! Sending email: sign-in codes and notices to guardians.

use async_trait::async_trait;
use lettre::message::Mailbox;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::Mail;

#[derive(Debug, thiserror::Error)]
#[error("the email could not be sent: {0}")]
pub struct MailError(pub String);

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError>;
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn new(settings: &Mail) -> Result<Self, MailError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(&settings.smtp_url)
            // The URL holds the password, so the error is not passed on.
            .map_err(|_| MailError("AULE_SMTP_URL is not a usable SMTP URL".into()))?
            .build();
        let from = settings.from.parse().map_err(|_| MailError("AULE_MAIL_FROM is not an address".into()))?;
        Ok(Self { transport, from })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(to.parse().map_err(|_| MailError("the recipient is not an address".into()))?)
            .subject(subject)
            .body(body.to_string())
            .map_err(|error| MailError(error.to_string()))?;
        self.transport.send(message).await.map_err(|error| MailError(error.to_string()))?;
        Ok(())
    }
}
