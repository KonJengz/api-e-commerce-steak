use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::shared::errors::AppError;
use crate::shared::slug::{append_slug_suffix, normalize_slug_lookup, slugify};

use super::model::*;

const CATEGORY_SLUG_FALLBACK: &str = "category";

/// Create a new category
pub async fn create_category(
    pool: &PgPool,
    req: &CreateCategoryRequest,
) -> Result<Category, AppError> {
    let normalized_name = normalize_category_name(&req.name)?;
    let normalized_description = normalize_optional_description(req.description.as_deref())?;

    let mut tx = pool.begin().await?;
    ensure_category_name_available(&mut *tx, &normalized_name, None).await?;

    let category_id = Uuid::now_v7();
    let slug = generate_unique_category_slug(&mut tx, &normalized_name, None).await?;

    let category = sqlx::query_as::<_, Category>(
        r#"INSERT INTO categories (id, slug, name, description, created_at, updated_at)
           VALUES ($1, $2, $3, $4, NOW(), NOW())
           RETURNING id, slug, name, description, created_at, updated_at"#,
    )
    .bind(category_id)
    .bind(slug)
    .bind(normalized_name)
    .bind(normalized_description)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(category)
}

/// List all categories (ordered by name)
pub async fn list_categories(pool: &PgPool) -> Result<Vec<Category>, AppError> {
    let categories = sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, description, created_at, updated_at FROM categories ORDER BY LOWER(name) ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(categories)
}

/// Get a single category by UUID, current slug, or historical slug.
pub async fn get_category(pool: &PgPool, identifier: &str) -> Result<Category, AppError> {
    if let Ok(category_id) = Uuid::parse_str(identifier)
        && let Some(category) = fetch_category_by_id(pool, category_id).await?
    {
        return Ok(category);
    }

    let slug = normalize_slug_lookup(identifier)
        .ok_or_else(|| AppError::NotFound("Category not found".to_string()))?;

    if let Some(category) = fetch_category_by_slug(pool, &slug).await? {
        return Ok(category);
    }

    sqlx::query_as::<_, Category>(
        r#"SELECT c.id, c.slug, c.name, c.description, c.created_at, c.updated_at
           FROM category_slug_history h
           INNER JOIN categories c ON c.id = h.category_id
           WHERE h.slug = $1"#,
    )
    .bind(&slug)
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

    let mut tx = pool.begin().await?;
    let existing_category = lock_category_for_update(&mut tx, category_id).await?;

    ensure_category_name_available(&mut *tx, &normalized_name, Some(category_id)).await?;

    let next_slug = if slugify(&normalized_name, CATEGORY_SLUG_FALLBACK) != existing_category.slug {
        Some(generate_unique_category_slug(&mut tx, &normalized_name, Some(category_id)).await?)
    } else {
        None
    };

    let category = sqlx::query_as::<_, Category>(
        r#"UPDATE categories
           SET slug = COALESCE($1, slug),
               name = $2,
               description = $3,
               updated_at = NOW()
           WHERE id = $4
           RETURNING id, slug, name, description, created_at, updated_at"#,
    )
    .bind(next_slug.as_deref())
    .bind(normalized_name)
    .bind(normalized_description)
    .bind(category_id)
    .fetch_one(&mut *tx)
    .await?;

    if next_slug.is_some() {
        save_category_slug_history(&mut tx, category_id, &existing_category.slug).await?;
    }

    tx.commit().await?;

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

async fn fetch_category_by_id(
    pool: &PgPool,
    category_id: Uuid,
) -> Result<Option<Category>, AppError> {
    sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, description, created_at, updated_at FROM categories WHERE id = $1",
    )
    .bind(category_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_category_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Category>, AppError> {
    sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, description, created_at, updated_at FROM categories WHERE slug = $1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)
}

async fn lock_category_for_update(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Uuid,
) -> Result<Category, AppError> {
    sqlx::query_as::<_, Category>(
        r#"SELECT id, slug, name, description, created_at, updated_at
           FROM categories
           WHERE id = $1
           FOR UPDATE"#,
    )
    .bind(category_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Category not found".to_string()))
}

async fn ensure_category_name_available<'e, E>(
    executor: E,
    name: &str,
    exclude_id: Option<Uuid>,
) -> Result<(), AppError>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_one(executor)
    .await?;

    if exists {
        return Err(AppError::BadRequest("Category already exists".to_string()));
    }

    Ok(())
}

async fn generate_unique_category_slug(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    exclude_category_id: Option<Uuid>,
) -> Result<String, AppError> {
    let base_slug = slugify(name, CATEGORY_SLUG_FALLBACK);
    let mut suffix = 1;

    loop {
        let candidate = append_slug_suffix(&base_slug, suffix);
        let exists_in_categories = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS(
                   SELECT 1
                   FROM categories
                   WHERE slug = $1
                     AND ($2::uuid IS NULL OR id <> $2)
               )"#,
        )
        .bind(&candidate)
        .bind(exclude_category_id)
        .fetch_one(&mut **tx)
        .await?;

        if !exists_in_categories {
            let exists_in_history = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM category_slug_history WHERE slug = $1)",
            )
            .bind(&candidate)
            .fetch_one(&mut **tx)
            .await?;

            if !exists_in_history {
                return Ok(candidate);
            }
        }

        suffix += 1;
    }
}

async fn save_category_slug_history(
    tx: &mut Transaction<'_, Postgres>,
    category_id: Uuid,
    previous_slug: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO category_slug_history (slug, category_id)
           VALUES ($1, $2)
           ON CONFLICT (slug) DO NOTHING"#,
    )
    .bind(previous_slug)
    .bind(category_id)
    .execute(&mut **tx)
    .await?;

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
