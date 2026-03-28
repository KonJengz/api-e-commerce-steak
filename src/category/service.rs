use sqlx::PgPool;
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::model::*;

/// Create a new category
pub async fn create_category(
    pool: &PgPool,
    req: &CreateCategoryRequest,
) -> Result<Category, AppError> {
    let normalized_name = normalize_category_name(&req.name)?;
    let normalized_description = normalize_optional_description(req.description.as_deref())?;

    ensure_category_name_available(pool, &normalized_name, None).await?;

    let category_id = Uuid::now_v7();

    let category = sqlx::query_as::<_, Category>(
        r#"INSERT INTO categories (id, name, description, created_at, updated_at)
           VALUES ($1, $2, $3, NOW(), NOW())
           RETURNING *"#,
    )
    .bind(category_id)
    .bind(normalized_name)
    .bind(normalized_description)
    .fetch_one(pool)
    .await?;

    Ok(category)
}

/// List all categories (ordered by name)
pub async fn list_categories(pool: &PgPool) -> Result<Vec<Category>, AppError> {
    let categories =
        sqlx::query_as::<_, Category>("SELECT * FROM categories ORDER BY LOWER(name) ASC")
            .fetch_all(pool)
            .await?;

    Ok(categories)
}

/// Get a single category by id
pub async fn get_category(pool: &PgPool, category_id: Uuid) -> Result<Category, AppError> {
    sqlx::query_as::<_, Category>("SELECT * FROM categories WHERE id = $1")
        .bind(category_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Category not found".to_string()))
}

/// Update an existing category
pub async fn update_category(
    pool: &PgPool,
    category_id: Uuid,
    req: &UpdateCategoryRequest,
) -> Result<Category, AppError> {
    let normalized_name = normalize_category_name(&req.name)?;
    let normalized_description = normalize_optional_description(req.description.as_deref())?;

    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1)")
            .bind(category_id)
            .fetch_one(pool)
            .await?;

    if !exists {
        return Err(AppError::NotFound("Category not found".to_string()));
    }

    ensure_category_name_available(pool, &normalized_name, Some(category_id)).await?;

    let category = sqlx::query_as::<_, Category>(
        r#"UPDATE categories
           SET name = $1,
               description = $2,
               updated_at = NOW()
           WHERE id = $3
           RETURNING *"#,
    )
    .bind(normalized_name)
    .bind(normalized_description)
    .bind(category_id)
    .fetch_one(pool)
    .await?;

    Ok(category)
}

/// Delete a category if no product is still assigned to it.
pub async fn delete_category(pool: &PgPool, category_id: Uuid) -> Result<(), AppError> {
    let assigned_products =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM products WHERE category_id = $1")
            .bind(category_id)
            .fetch_one(pool)
            .await?;

    if assigned_products > 0 {
        return Err(AppError::Conflict(
            "Cannot delete category while products are assigned to it".to_string(),
        ));
    }

    let result = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(category_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Category not found".to_string()));
    }

    Ok(())
}

async fn ensure_category_name_available(
    pool: &PgPool,
    name: &str,
    exclude_id: Option<Uuid>,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS(
               SELECT 1
               FROM categories
               WHERE LOWER(name) = LOWER($1)
                 AND ($2::uuid IS NULL OR id <> $2)
           )"#,
    )
    .bind(name)
    .bind(exclude_id)
    .fetch_one(pool)
    .await?;

    if exists {
        return Err(AppError::BadRequest("Category already exists".to_string()));
    }

    Ok(())
}

fn normalize_category_name(name: &str) -> Result<String, AppError> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "Category name is required".to_string(),
        ));
    }

    if trimmed.chars().count() > 255 {
        return Err(AppError::BadRequest(
            "Category name must be at most 255 characters".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

fn normalize_optional_description(description: Option<&str>) -> Result<Option<String>, AppError> {
    match description {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();

            if trimmed.is_empty() {
                return Ok(None);
            }

            if trimmed.chars().count() > 2000 {
                return Err(AppError::BadRequest(
                    "Description must be at most 2000 characters".to_string(),
                ));
            }

            Ok(Some(trimmed.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_category_name_trims_input() {
        assert_eq!(
            normalize_category_name("  Smartphones  ").expect("name should normalize"),
            "Smartphones"
        );
    }

    #[test]
    fn normalize_category_name_rejects_blank_values() {
        assert!(matches!(
            normalize_category_name("   "),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn normalize_optional_description_drops_blank_strings() {
        assert_eq!(
            normalize_optional_description(Some("   ")).expect("description should normalize"),
            None
        );
        assert_eq!(
            normalize_optional_description(Some("  Mobile devices  "))
                .expect("description should normalize"),
            Some("Mobile devices".to_string())
        );
    }
}
