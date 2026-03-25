use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::{delete, get, post, put},
};
use uuid::Uuid;
use validator::Validate;

use crate::AppState;
use crate::shared::cloudinary;
use crate::shared::errors::AppError;
use crate::shared::extractors::AdminUser;
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::*;
use super::service;

const MAX_IMAGE_SIZE_BYTES: usize = 5 * 1024 * 1024;
const MAX_MULTIPART_BODY_SIZE_BYTES: usize = 6 * 1024 * 1024;
const ALLOWED_IMAGE_CONTENT_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];

pub fn router() -> Router<AppState> {
    let upload_router = Router::new()
        .route("/upload-image", post(upload_image))
        .layer(DefaultBodyLimit::max(MAX_MULTIPART_BODY_SIZE_BYTES));

    Router::new()
        .route("/", get(list_products).post(create_product))
        .route("/{id}/image", delete(delete_primary_product_image))
        .route(
            "/{id}/images",
            get(list_product_images).post(attach_product_image),
        )
        .route("/{id}/images/reorder", put(reorder_product_images))
        .route("/{id}/images/{image_id}", delete(delete_product_image))
        .route(
            "/{id}",
            get(get_product).put(update_product).delete(delete_product),
        )
        .merge(upload_router)
}

/// GET /api/products (public)
async fn list_products(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Product>>, AppError> {
    let products = service::list_products(&state.pool, query).await?;
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

/// GET /api/products/:id/images (public)
async fn list_product_images(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProductImage>>, AppError> {
    let _product = service::get_product(&state.pool, id).await?;
    let images = service::list_product_images(&state.pool, id).await?;
    Ok(Json(images))
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

        if let (Some(old_public_id), Some(new_public_id)) = (old_public_id, new_public_id)
            && old_public_id != new_public_id
        {
            delete_cloudinary_image_or_queue(
                &state,
                product.id,
                &old_public_id,
                "replace_primary_image",
            )
            .await;
        }
    }

    Ok(Json(product))
}

/// POST /api/products/:id/images (admin only)
async fn attach_product_image(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AttachProductImageRequest>,
) -> Result<Json<ProductImageMutationResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    validate_image_fields(Some(&body.image_url), Some(&body.image_public_id))?;

    let response =
        service::attach_product_image(&state.pool, id, &body, admin.user_id, &state.config).await?;

    Ok(Json(response))
}

/// PUT /api/products/:id/images/reorder (admin only)
async fn reorder_product_images(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ReorderProductImagesRequest>,
) -> Result<Json<ProductImageMutationResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let response = service::reorder_product_images(&state.pool, id, &body).await?;
    Ok(Json(response))
}

/// DELETE /api/products/:id/image (admin only)
async fn delete_primary_product_image(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductImageMutationResponse>, AppError> {
    let (response, deleted_public_id) =
        service::clear_primary_product_image(&state.pool, id).await?;

    if let Some(public_id) = deleted_public_id.as_deref() {
        delete_cloudinary_image_or_queue(&state, id, public_id, "delete_primary_image").await;
    }

    Ok(Json(response))
}

/// DELETE /api/products/:id/images/:image_id (admin only)
async fn delete_product_image(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path((id, image_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProductImageMutationResponse>, AppError> {
    let (response, deleted_public_id) =
        service::delete_product_image(&state.pool, id, image_id).await?;

    if let Some(public_id) = deleted_public_id.as_deref() {
        delete_cloudinary_image_or_queue(&state, id, public_id, "delete_gallery_image").await;
    }

    Ok(Json(response))
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

    loop {
        let Some(field) = multipart.next_field().await.map_err(map_multipart_error)? else {
            break;
        };

        if field.name() == Some("image") {
            file_name = field.file_name().unwrap_or("image.jpg").to_string();
            content_type = field.content_type().unwrap_or("image/jpeg").to_string();
            file_data = field.bytes().await.map_err(map_multipart_error)?.to_vec();
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

fn map_multipart_error(error: axum::extract::multipart::MultipartError) -> AppError {
    let message = error.to_string();

    if message.contains("length limit exceeded") || message.contains("body too large") {
        return AppError::BadRequest(format!(
            "Image upload is too large. Maximum request size is {} MB.",
            MAX_MULTIPART_BODY_SIZE_BYTES / 1024 / 1024
        ));
    }

    AppError::BadRequest(
        "Failed to parse multipart/form-data request. Make sure you send a FormData body with an `image` file field.".to_string(),
    )
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

async fn delete_cloudinary_image_or_queue(
    state: &AppState,
    product_id: Uuid,
    public_id: &str,
    operation: &str,
) {
    if public_id.trim().is_empty() {
        return;
    }

    if let Err(error) = cloudinary::delete_image(public_id, &state.config).await {
        tracing::error!(
            error = %error,
            product_id = %product_id,
            public_id = %public_id,
            operation = operation,
            "failed to delete product image from Cloudinary"
        );

        if let Err(queue_error) =
            service::enqueue_pending_product_image_deletion(&state.pool, public_id).await
        {
            tracing::error!(
                error = %queue_error,
                product_id = %product_id,
                public_id = %public_id,
                operation = operation,
                "failed to queue product image for retry deletion"
            );
        }
    }
}
