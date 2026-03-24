use axum::{
    extract::{Multipart, Path, State},
    routing::{get, post},
    Json, Router,
};
use uuid::Uuid;
use validator::Validate;

use crate::shared::cloudinary;
use crate::shared::errors::AppError;
use crate::shared::extractors::AdminUser;
use crate::AppState;

use super::model::*;
use super::service;

const MAX_IMAGE_SIZE_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_IMAGE_CONTENT_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_products).post(create_product))
        .route("/upload-image", post(upload_image))
        .route(
            "/{id}",
            get(get_product).put(update_product).delete(delete_product),
        )
}

/// GET /api/products (public)
async fn list_products(State(state): State<AppState>) -> Result<Json<Vec<Product>>, AppError> {
    let products = service::list_products(&state.pool).await?;
    Ok(Json(products))
}

/// GET /api/products/:id (public)
async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Product>, AppError> {
    let product = service::get_product(&state.pool, id).await?;
    Ok(Json(product))
}

/// POST /api/products (admin only)
async fn create_product(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(body): Json<CreateProductRequest>,
) -> Result<Json<Product>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    validate_image_fields(body.image_url.as_deref(), body.image_public_id.as_deref())?;

    let product = service::create_product(&state.pool, &body, admin.user_id, &state.config).await?;
    Ok(Json(product))
}

/// PUT /api/products/:id (admin only)
async fn update_product(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProductRequest>,
) -> Result<Json<Product>, AppError> {
    validate_image_fields(body.image_url.as_deref(), body.image_public_id.as_deref())?;

    let previous_product = if body.image_url.is_some() || body.image_public_id.is_some() {
        Some(service::get_product_for_admin(&state.pool, id).await?)
    } else {
        None
    };

    let product =
        service::update_product(&state.pool, id, &body, admin.user_id, &state.config).await?;

    if let Some(previous_product) = previous_product {
        let old_public_id = previous_product.image_public_id;
        let new_public_id = product.image_public_id.clone();

        if let (Some(old_public_id), Some(new_public_id)) = (old_public_id, new_public_id) {
            if old_public_id != new_public_id {
                if let Err(error) = cloudinary::delete_image(&old_public_id, &state.config).await {
                    tracing::error!(
                        error = %error,
                        product_id = %product.id,
                        public_id = %old_public_id,
                        "failed to delete replaced product image from Cloudinary"
                    );

                    if let Err(queue_error) =
                        service::enqueue_pending_product_image_deletion(&state.pool, &old_public_id)
                            .await
                    {
                        tracing::error!(
                            error = %queue_error,
                            product_id = %product.id,
                            public_id = %old_public_id,
                            "failed to queue product image for retry deletion"
                        );
                    }
                }
            }
        }
    }

    Ok(Json(product))
}

/// DELETE /api/products/:id (admin only — soft delete)
async fn delete_product(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    service::delete_product(&state.pool, id).await?;
    Ok(Json(
        serde_json::json!({"message": "Product deleted successfully"}),
    ))
}

/// POST /api/products/upload-image (admin only)
async fn upload_image(
    State(state): State<AppState>,
    admin: AdminUser,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut file_data = Vec::new();
    let mut file_name = String::new();
    let mut content_type = String::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Failed to parse multipart: {}", e)))?
    {
        if field.name() == Some("image") {
            file_name = field.file_name().unwrap_or("image.jpg").to_string();
            content_type = field.content_type().unwrap_or("image/jpeg").to_string();
            file_data = field
                .bytes()
                .await
                .map_err(|e| AppError::BadRequest(format!("Failed to read file: {}", e)))?
                .to_vec();
        }
    }

    if file_data.is_empty() {
        return Err(AppError::BadRequest("No image file uploaded".to_string()));
    }

    validate_uploaded_image(&file_data, &content_type)?;

    let image =
        cloudinary::upload_image(&file_name, file_data, &content_type, &state.config).await?;

    if let Err(error) = service::save_pending_product_image(
        &state.pool,
        &image.public_id,
        &image.secure_url,
        admin.user_id,
        state.config.product_image_upload_ttl_minutes,
    )
    .await
    {
        if let Err(cleanup_error) = cloudinary::delete_image(&image.public_id, &state.config).await
        {
            tracing::error!(
                error = %cleanup_error,
                public_id = %image.public_id,
                "failed to roll back Cloudinary upload after database error"
            );

            if let Err(queue_error) =
                service::enqueue_pending_product_image_deletion(&state.pool, &image.public_id).await
            {
                tracing::error!(
                    error = %queue_error,
                    public_id = %image.public_id,
                    "failed to queue rolled back Cloudinary upload for retry deletion"
                );
            }
        }

        return Err(error);
    }

    Ok(Json(serde_json::json!({
        "image_url": image.secure_url,
        "image_public_id": image.public_id,
    })))
}

fn validate_uploaded_image(file_data: &[u8], content_type: &str) -> Result<(), AppError> {
    if file_data.len() > MAX_IMAGE_SIZE_BYTES {
        return Err(AppError::BadRequest(format!(
            "Image is too large. Maximum size is {} MB.",
            MAX_IMAGE_SIZE_BYTES / 1024 / 1024
        )));
    }

    if !ALLOWED_IMAGE_CONTENT_TYPES.contains(&content_type) {
        return Err(AppError::BadRequest(
            "Unsupported image type. Allowed types: image/jpeg, image/png, image/webp".to_string(),
        ));
    }

    Ok(())
}

fn validate_image_fields(
    image_url: Option<&str>,
    image_public_id: Option<&str>,
) -> Result<(), AppError> {
    match (image_url, image_public_id) {
        (None, None) => Ok(()),
        (Some(image_url), Some(image_public_id))
            if !image_url.trim().is_empty() && !image_public_id.trim().is_empty() =>
        {
            Ok(())
        }
        _ => Err(AppError::BadRequest(
            "image_url and image_public_id must both be provided together".to_string(),
        )),
    }
}
