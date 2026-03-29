use reqwest::multipart;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::config::AppConfig;
use crate::shared::errors::AppError;

const PRODUCT_IMAGES_FOLDER: &str = "products";
const USER_IMAGES_FOLDER: &str = "users";
const ORDER_PAYMENT_SLIPS_FOLDER: &str = "orders/payment-slips";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UploadedImage {
    pub public_id: String,
    pub secure_url: String,
}

#[derive(Deserialize)]
struct CloudinaryDestroyResponse {
    result: String,
}

/// Uploads an image to Cloudinary using their REST API
/// Generates a signed payload so the frontend doesn't need to know the secret
pub async fn upload_image(
    file_name: &str,
    file_data: Vec<u8>,
    content_type: &str,
    config: &AppConfig,
) -> Result<UploadedImage, AppError> {
    upload_image_to_folder(
        PRODUCT_IMAGES_FOLDER,
        file_name,
        file_data,
        content_type,
        config,
    )
    .await
}

pub async fn upload_user_image(
    file_name: &str,
    file_data: Vec<u8>,
    content_type: &str,
    config: &AppConfig,
) -> Result<UploadedImage, AppError> {
    upload_image_to_folder(
        USER_IMAGES_FOLDER,
        file_name,
        file_data,
        content_type,
        config,
    )
    .await
}

pub async fn upload_order_payment_slip(
    file_name: &str,
    file_data: Vec<u8>,
    content_type: &str,
    config: &AppConfig,
) -> Result<UploadedImage, AppError> {
    upload_image_to_folder(
        ORDER_PAYMENT_SLIPS_FOLDER,
        file_name,
        file_data,
        content_type,
        config,
    )
    .await
}

async fn upload_image_to_folder(
    folder: &str,
    file_name: &str,
    file_data: Vec<u8>,
    content_type: &str,
    config: &AppConfig,
) -> Result<UploadedImage, AppError> {
    let url = format!(
        "https://api.cloudinary.com/v1_1/{}/image/upload",
        config.cloudinary_cloud_name
    );

    let timestamp = chrono::Utc::now().timestamp().to_string();
    let signature = sign_params(
        &[("folder", folder), ("timestamp", &timestamp)],
        &config.cloudinary_api_secret,
    );

    let part = multipart::Part::bytes(file_data)
        .file_name(file_name.to_owned())
        .mime_str(content_type)
        .map_err(|e| AppError::Internal(format!("Invalid mime type for image: {}", e)))?;

    let form = multipart::Form::new()
        .part("file", part)
        .text("folder", folder.to_string())
        .text("api_key", config.cloudinary_api_key.clone())
        .text("timestamp", timestamp)
        .text("signature", signature);

    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to upload to Cloudinary: {}", e)))?;

    if !res.status().is_success() {
        let error_body = res.text().await.unwrap_or_default();
        tracing::error!("Cloudinary upload error: {}", error_body);
        return Err(AppError::Internal("Cloudinary upload failed".to_string()));
    }

    let response: UploadedImage = res
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse Cloudinary response: {}", e)))?;

    Ok(response)
}

/// Deletes an image from Cloudinary by public_id.
pub async fn delete_image(public_id: &str, config: &AppConfig) -> Result<(), AppError> {
    if public_id.trim().is_empty() {
        return Ok(());
    }

    let url = format!(
        "https://api.cloudinary.com/v1_1/{}/image/destroy",
        config.cloudinary_cloud_name
    );
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let signature = sign_params(
        &[
            ("invalidate", "true"),
            ("public_id", public_id),
            ("timestamp", &timestamp),
        ],
        &config.cloudinary_api_secret,
    );

    let form = multipart::Form::new()
        .text("public_id", public_id.to_string())
        .text("invalidate", "true".to_string())
        .text("api_key", config.cloudinary_api_key.clone())
        .text("timestamp", timestamp)
        .text("signature", signature);

    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to delete Cloudinary image: {}", e)))?;

    if !res.status().is_success() {
        let error_body = res.text().await.unwrap_or_default();
        tracing::error!("Cloudinary delete error: {}", error_body);
        return Err(AppError::Internal("Cloudinary delete failed".to_string()));
    }

    let response: CloudinaryDestroyResponse = res.json().await.map_err(|e| {
        AppError::Internal(format!("Failed to parse Cloudinary delete response: {}", e))
    })?;

    if response.result == "ok" || response.result == "not found" {
        Ok(())
    } else {
        Err(AppError::Internal(format!(
            "Unexpected Cloudinary delete result: {}",
            response.result
        )))
    }
}

fn sign_params(params: &[(&str, &str)], api_secret: &str) -> String {
    let mut pairs = params
        .iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>();
    pairs.sort();

    let string_to_sign = format!("{}{}", pairs.join("&"), api_secret);
    let mut hasher = Sha1::new();
    hasher.update(string_to_sign.as_bytes());
    hex::encode(hasher.finalize())
}
