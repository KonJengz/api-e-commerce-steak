use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};

use crate::config::AppConfig;
use crate::shared::errors::AppError;

/// Send a verification email with a 6-digit code
pub async fn send_verification_email(
    to: &str,
    code: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let subject = "Verify your email address";
    let body = format!(
        r#"<html>
<body style="font-family: Arial, sans-serif; padding: 20px;">
    <h2>Email Verification</h2>
    <p>Thank you for registering! Please use the following code to verify your email:</p>
    <div style="background: #f0f0f0; padding: 20px; text-align: center; font-size: 32px; font-weight: bold; letter-spacing: 8px; margin: 20px 0;">
        {}
    </div>
    <p>This code will expire in 15 minutes.</p>
    <p>If you didn't request this, please ignore this email.</p>
</body>
</html>"#,
        code
    );

    send_email(to, subject, &body, config).await
}

/// Send a welcome email after successful verification
pub async fn send_welcome_email(to: &str, config: &AppConfig) -> Result<(), AppError> {
    let subject = "Welcome to our store!";
    let body = r#"<html>
<body style="font-family: Arial, sans-serif; padding: 20px;">
    <h2>Welcome! 🎉</h2>
    <p>Your email has been verified and your account is now active.</p>
    <p>Start exploring our products and enjoy shopping!</p>
</body>
</html>"#
        .to_string();

    send_email(to, subject, &body, config).await
}

/// Send a login notification email
pub async fn send_login_notification(to: &str, config: &AppConfig) -> Result<(), AppError> {
    let subject = "New login to your account";
    let body = r#"<html>
<body style="font-family: Arial, sans-serif; padding: 20px;">
    <h2>Login Notification</h2>
    <p>A new login to your account was detected.</p>
    <p>If this wasn't you, please change your password immediately.</p>
</body>
</html>"#
        .to_string();

    send_email(to, subject, &body, config).await
}

/// Internal helper to send an email via SMTP
async fn send_email(
    to: &str,
    subject: &str,
    html_body: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let email = Message::builder()
        .from(config.smtp_from.parse().map_err(|e| {
            AppError::Internal(format!("Invalid from address: {}", e))
        })?)
        .to(to.parse().map_err(|e| {
            AppError::Internal(format!("Invalid to address: {}", e))
        })?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html_body.to_string())?;

    let creds = Credentials::new(
        config.smtp_username.clone(),
        config.smtp_password.clone(),
    );

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
        .map_err(|e| AppError::Internal(format!("SMTP relay error: {}", e)))?
        .credentials(creds)
        .port(config.smtp_port)
        .build();

    mailer.send(email).await?;

    Ok(())
}
