use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::*;

/// Create a new category
pub async fn create_category(
    pool: &PgPool,
    req: &CreateCategoryRequest,
) -> Result<Category, AppError> {
    // Check if category name already exists
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM categories WHERE name = $1)")
            .bind(&req.name)
            .fetch_one(pool)
            .await?;

    if exists {
        return Err(AppError::BadRequest("Category already exists".to_string()));
    }

    let category_id = Uuid::now_v7();

    let category = sqlx::query_as::<_, Category>(
        r#"INSERT INTO categories (id, name, description, created_at, updated_at)
           VALUES ($1, $2, $3, NOW(), NOW())
           RETURNING *"#,
    )
    .bind(category_id)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_one(pool)
    .await?;

    Ok(category)
}

/// List all categories (ordered by name)
pub async fn list_categories(pool: &PgPool) -> Result<Vec<Category>, AppError> {
    let categories = sqlx::query_as::<_, Category>("SELECT * FROM categories ORDER BY name ASC")
        .fetch_all(pool)
        .await?;

    Ok(categories)
}
