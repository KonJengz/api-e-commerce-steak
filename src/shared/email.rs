use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::shared::errors::AppError;

const RESEND_EMAILS_API_URL: &str = "https://api.resend.com/emails";

#[derive(Debug, Serialize)]
struct ResendEmailRequest<'a> {
    from: &'a str,
    to: Vec<&'a str>,
    subject: &'a str,
    html: &'a str,
}

#[derive(Debug, Deserialize)]
struct ResendEmailResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ResendErrorResponse {
    message: Option<String>,
    name: Option<String>,
}

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

/// Send an order confirmation email
pub async fn send_order_confirmation(
    to: &str,
    order_id: &str,
    total: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let subject = format!("Order Confirmation - #{}", order_id);
    let body = format!(
        r#"<html>
<body style="font-family: Arial, sans-serif; padding: 20px; line-height: 1.6;">
    <h2 style="color: #4f46e5;">Thank you for your order! 🎉</h2>
    <p>Your order <strong>#{}</strong> has been successfully placed.</p>
    <div style="background: #f8fafc; padding: 15px; border-radius: 8px; margin: 20px 0;">
        <p style="margin: 0;">Total amount paid: <strong style="font-size: 18px; color: #0f172a;">฿{}</strong></p>
    </div>
    <p>We will notify you once your order is shipped.</p>
</body>
</html>"#,
        order_id, total
    );

    send_email(to, &subject, &body, config).await
}

/// Internal helper to send an email via Resend API
async fn send_email(
    to: &str,
    subject: &str,
    html_body: &str,
    config: &AppConfig,
) -> Result<(), AppError> {
    let client = reqwest::Client::new();
    let payload = ResendEmailRequest {
        from: config.email_from.as_str(),
        to: vec![to],
        subject,
        html: html_body,
    };

    let response = client
        .post(RESEND_EMAILS_API_URL)
        .header(AUTHORIZATION, format!("Bearer {}", config.resend_api_key))
        .header(
            USER_AGENT,
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
        )
        .header(CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|error| {
            tracing::error!(error = ?error, "failed to reach Resend API");
            AppError::Internal("Failed to reach Resend API".to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response
            .json::<ResendErrorResponse>()
            .await
            .ok()
            .and_then(|body| body.message.or(body.name))
            .unwrap_or_else(|| "Unknown Resend API error".to_string());

        tracing::error!(
            status = %status,
            error = %error_body,
            "Resend API returned an error response"
        );

        return Err(AppError::Internal(
            "Failed to send email via Resend".to_string(),
        ));
    }

    let response_body = response
        .json::<ResendEmailResponse>()
        .await
        .map_err(|error| {
            tracing::error!(error = ?error, "failed to parse Resend API response");
            AppError::Internal("Failed to parse Resend API response".to_string())
        })?;

    tracing::info!(email_id = %response_body.id, to = %to, "email sent via Resend");

    Ok(())
}
