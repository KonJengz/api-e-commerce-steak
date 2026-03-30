use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::get,
};
use uuid::Uuid;
use validator::Validate;

use crate::AppState;
use crate::shared::background::spawn_app_task;
use crate::shared::cloudinary;
use crate::shared::email;
use crate::shared::errors::AppError;
use crate::shared::extractors::{AdminUser, AuthUser};
use crate::shared::pagination::{PaginatedResponse, PaginationQuery};

use super::model::*;
use super::service;

const MAX_IMAGE_SIZE_BYTES: usize = 5 * 1024 * 1024;
const MAX_MULTIPART_BODY_SIZE_BYTES: usize = 6 * 1024 * 1024;
const ALLOWED_IMAGE_CONTENT_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];

#[derive(Debug)]
struct UploadedPaymentSlip {
    file_name: String,
    content_type: String,
    file_data: Vec<u8>,
}

pub fn router() -> Router<AppState> {
    let payment_slip_router = Router::new()
        .route(
            "/{id}/payment-slip",
            axum::routing::put(update_order_payment_slip),
        )
        .layer(DefaultBodyLimit::max(MAX_MULTIPART_BODY_SIZE_BYTES));

    Router::new()
        .route("/admin", get(list_orders_for_admin))
        .route(
            "/admin/{id}",
            get(get_order_for_admin).put(update_order_for_admin),
        )
        .route("/", get(list_orders).post(create_order))
        .route("/{id}", get(get_order))
        .merge(payment_slip_router)
}

/// POST /api/orders
/// Create a new order — snapshots price from products at purchase time
async fn create_order(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateOrderRequest>,
) -> Result<Json<OrderResponse>, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let user_email = sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_one(&state.pool)
        .await?;

    let order = service::create_order(&state.pool, auth.user_id, &body).await?;

    // Send order confirmation email in background
    let config = state.config.clone();
    let order_id = order.id.to_string();
    let total = order.total_amount.to_string();

    spawn_app_task("send_order_confirmation", async move {
        email::send_order_confirmation(&user_email, &order_id, &total, &config).await
    });

    Ok(Json(order))
}

/// GET /api/orders
async fn list_orders(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<OrderListItemResponse>>, AppError> {
    let orders = service::list_orders(&state.pool, auth.user_id, query).await?;
    Ok(Json(orders))
}

/// GET /api/orders/admin
async fn list_orders_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<AdminOrderListQuery>,
) -> Result<Json<AdminOrderListResponse>, AppError> {
    let orders = service::list_orders_for_admin(&state.pool, query).await?;
    Ok(Json(orders))
}

/// GET /api/orders/:id
async fn get_order(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderResponse>, AppError> {
    let order = service::get_order(&state.pool, auth.user_id, id).await?;
    Ok(Json(order))
}

/// PUT /api/orders/:id/payment-slip
async fn update_order_payment_slip(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<OrderResponse>, AppError> {
    let mut slip: Option<UploadedPaymentSlip> = None;

    loop {
        let Some(field) = multipart.next_field().await.map_err(map_multipart_error)? else {
            break;
        };

        match field.name() {
            Some("slip") => {
                let file_name = field.file_name().unwrap_or("payment-slip.jpg").to_string();
                let content_type = field.content_type().unwrap_or("image/jpeg").to_string();
                let file_data = field.bytes().await.map_err(map_multipart_error)?.to_vec();

                validate_uploaded_image(&file_data, &content_type)?;

                slip = Some(UploadedPaymentSlip {
                    file_name,
                    content_type,
                    file_data,
                });
            }
            _ => {
                let _ = field.bytes().await.map_err(map_multipart_error)?;
            }
        }
    }

    let slip = slip.ok_or_else(|| {
        AppError::BadRequest("Provide a `slip` image file in multipart/form-data".to_string())
    })?;

    let uploaded_slip = cloudinary::upload_order_payment_slip(
        &slip.file_name,
        slip.file_data,
        &slip.content_type,
        &state.config,
    )
    .await?;

    let update_result = service::update_order_payment_slip(
        &state.pool,
        auth.user_id,
        id,
        &uploaded_slip.secure_url,
        &uploaded_slip.public_id,
    )
    .await;

    let (order, previous_payment_slip_public_id) = match update_result {
        Ok(result) => result,
        Err(error) => {
            if let Err(cleanup_error) =
                cloudinary::delete_image(&uploaded_slip.public_id, &state.config).await
            {
                tracing::error!(
                    error = %cleanup_error,
                    order_id = %id,
                    public_id = %uploaded_slip.public_id,
                    "failed to roll back uploaded payment slip after database error"
                );
            }

            return Err(error);
        }
    };

    if let Some(previous_payment_slip_public_id) = previous_payment_slip_public_id.as_deref()
        && previous_payment_slip_public_id != uploaded_slip.public_id
        && let Err(error) =
            cloudinary::delete_image(previous_payment_slip_public_id, &state.config).await
    {
        tracing::error!(
            error = %error,
            order_id = %id,
            public_id = %previous_payment_slip_public_id,
            "failed to delete replaced payment slip from Cloudinary"
        );
    }

    Ok(Json(order))
}

/// GET /api/orders/admin/:id
async fn get_order_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<AdminOrderResponse>, AppError> {
    let order = service::get_order_for_admin(&state.pool, id).await?;
    Ok(Json(order))
}

/// PUT /api/orders/admin/:id
async fn update_order_for_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateOrderRequest>,
) -> Result<Json<AdminOrderResponse>, AppError> {
    body.validate()
        .map_err(|error| AppError::BadRequest(error.to_string()))?;

    let (order, notification) = service::update_order_for_admin(&state.pool, id, &body).await?;

    if let Some(notification) = notification {
        let config = state.config.clone();

        spawn_app_task("send_order_tracking_email", async move {
            email::send_order_tracking_email(
                &notification.user_email,
                &notification.order_id,
                &notification.tracking_number,
                &config,
            )
            .await
        });
    }

    Ok(Json(order))
}

fn validate_uploaded_image(file_data: &[u8], content_type: &str) -> Result<(), AppError> {
    if file_data.is_empty() {
        return Err(AppError::BadRequest("No image file uploaded".to_string()));
    }

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
        "Failed to parse multipart/form-data request. Make sure you send a FormData body with a `slip` image field.".to_string(),
    )
}
