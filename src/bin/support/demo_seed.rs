#![allow(dead_code)]

use std::collections::HashMap;
use std::io;

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

type SeedResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct SeedUser {
    id: &'static str,
    name: &'static str,
    email: &'static str,
    password: &'static str,
    role: &'static str,
    image: Option<&'static str>,
}

struct SeedAddress {
    id: &'static str,
    user_id: &'static str,
    recipient_name: &'static str,
    phone: Option<&'static str>,
    address_line: &'static str,
    city: &'static str,
    postal_code: &'static str,
    is_default: bool,
}

struct SeedCategory {
    id: &'static str,
    slug: &'static str,
    name: &'static str,
    description: &'static str,
}

struct SeedProduct {
    id: &'static str,
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    category_id: &'static str,
    image_url: &'static str,
    image_public_id: &'static str,
    current_price: &'static str,
    stock: i32,
    is_active: bool,
}

#[derive(Clone)]
struct AddressSnapshot {
    recipient_name: String,
    phone: Option<String>,
    address_line: String,
    city: String,
    postal_code: String,
}

#[derive(Clone)]
struct ProductSnapshot {
    id: Uuid,
    name: String,
    price: Decimal,
}

struct SeedOrderLine {
    product_id: &'static str,
    quantity: i32,
}

struct SeedOrderInput {
    id: &'static str,
    user_id: &'static str,
    address_id: &'static str,
    status: &'static str,
    tracking_number: Option<&'static str>,
    payment_slip_url: Option<&'static str>,
    payment_slip_public_id: Option<&'static str>,
    payment_submitted_at: Option<chrono::DateTime<Utc>>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    items: Vec<SeedOrderLine>,
}

const DEMO_USERS: &[SeedUser] = &[
    SeedUser {
        id: "10000000-0000-4000-8000-000000000001",
        name: "Steak Box Admin",
        email: "admin@steakbox.dev",
        password: "Admin12345!",
        role: "ADMIN",
        image: Some("https://images.unsplash.com/photo-1500648767791-00dcc994a43e"),
    },
    SeedUser {
        id: "10000000-0000-4000-8000-000000000002",
        name: "Niran Chef",
        email: "chef@steakbox.dev",
        password: "SteakBox123!",
        role: "USER",
        image: Some("https://images.unsplash.com/photo-1506794778202-cad84cf45f1d"),
    },
    SeedUser {
        id: "10000000-0000-4000-8000-000000000003",
        name: "May Homecook",
        email: "homecook@steakbox.dev",
        password: "SteakBox123!",
        role: "USER",
        image: Some("https://images.unsplash.com/photo-1494790108377-be9c29b29330"),
    },
];

const DEMO_ADDRESSES: &[SeedAddress] = &[
    SeedAddress {
        id: "20000000-0000-4000-8000-000000000001",
        user_id: "10000000-0000-4000-8000-000000000002",
        recipient_name: "Niran Chef",
        phone: Some("0812345678"),
        address_line: "88/12 Sukhumvit 24, Khlong Tan",
        city: "Bangkok",
        postal_code: "10110",
        is_default: true,
    },
    SeedAddress {
        id: "20000000-0000-4000-8000-000000000002",
        user_id: "10000000-0000-4000-8000-000000000002",
        recipient_name: "Niran Chef",
        phone: Some("0812345678"),
        address_line: "17/4 Nimman Road, Suthep",
        city: "Chiang Mai",
        postal_code: "50200",
        is_default: false,
    },
    SeedAddress {
        id: "20000000-0000-4000-8000-000000000003",
        user_id: "10000000-0000-4000-8000-000000000003",
        recipient_name: "May Homecook",
        phone: Some("0898765432"),
        address_line: "119/6 Ladprao 71, Wang Thonglang",
        city: "Bangkok",
        postal_code: "10310",
        is_default: true,
    },
];

const DEMO_CATEGORIES: &[SeedCategory] = &[
    SeedCategory {
        id: "30000000-0000-4000-8000-000000000001",
        slug: "signature-steaks",
        name: "Signature Steaks",
        description: "Chef-picked center cuts for pan searing, butter basting, and charcoal grilling.",
    },
    SeedCategory {
        id: "30000000-0000-4000-8000-000000000002",
        slug: "wagyu-selection",
        name: "Wagyu Selection",
        description: "Marbled wagyu cuts for guests who want a richer finish and softer bite.",
    },
    SeedCategory {
        id: "30000000-0000-4000-8000-000000000003",
        slug: "grill-ready",
        name: "Grill Ready",
        description: "Fast-moving cuts built for yakiniku nights, quick grilling, and easy home service.",
    },
    SeedCategory {
        id: "30000000-0000-4000-8000-000000000004",
        slug: "slow-cook-bbq",
        name: "Slow Cook & BBQ",
        description: "Heavier cuts for weekend smoking, oven roasting, and crowd-feeding barbecue sessions.",
    },
];

const DEMO_PRODUCTS: &[SeedProduct] = &[
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000001",
        slug: "australian-wagyu-ribeye-mbs67",
        name: "Australian Wagyu Ribeye MBS 6/7",
        description: "Rich marbling with a clean beef finish. Ideal for high-heat searing and quick resting before service.",
        category_id: "30000000-0000-4000-8000-000000000002",
        image_url: "https://images.unsplash.com/photo-1544025162-d76694265947",
        image_public_id: "seed/steak-box/australian-wagyu-ribeye-mbs67",
        current_price: "1890.00",
        stock: 12,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000002",
        slug: "dry-aged-striploin-30-day",
        name: "Dry-Aged Striploin 30 Day",
        description: "A nutty, deeper striploin profile with enough fat cap to hold moisture during a hard sear.",
        category_id: "30000000-0000-4000-8000-000000000001",
        image_url: "https://images.unsplash.com/photo-1551024506-0bccd828d307",
        image_public_id: "seed/steak-box/dry-aged-striploin-30-day",
        current_price: "1690.00",
        stock: 8,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000003",
        slug: "grass-fed-tenderloin-center-cut",
        name: "Grass-Fed Tenderloin Center Cut",
        description: "Lean, delicate, and trimmed for clean plating. Best for fast service and classic steakhouse sauces.",
        category_id: "30000000-0000-4000-8000-000000000001",
        image_url: "https://images.unsplash.com/photo-1600891964092-4316c288032e",
        image_public_id: "seed/steak-box/grass-fed-tenderloin-center-cut",
        current_price: "1490.00",
        stock: 10,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000004",
        slug: "picanha-steak",
        name: "Picanha Steak",
        description: "Brazilian-style cap steak with bold fat cover and quick-fire grill performance.",
        category_id: "30000000-0000-4000-8000-000000000003",
        image_url: "https://images.unsplash.com/photo-1512152272829-e3139592d56f",
        image_public_id: "seed/steak-box/picanha-steak",
        current_price: "990.00",
        stock: 18,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000005",
        slug: "korean-galbi-short-ribs",
        name: "Korean Galbi Short Ribs",
        description: "Thin-cut beef ribs for Korean barbecue, ready for quick marinades and table grilling.",
        category_id: "30000000-0000-4000-8000-000000000003",
        image_url: "https://images.unsplash.com/photo-1544025162-d76694265947",
        image_public_id: "seed/steak-box/korean-galbi-short-ribs",
        current_price: "1290.00",
        stock: 14,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000006",
        slug: "shabu-short-plate",
        name: "Shabu Short Plate",
        description: "Thin-sliced beef for hotpot, rice bowls, and weeknight stir-fry service.",
        category_id: "30000000-0000-4000-8000-000000000003",
        image_url: "https://images.unsplash.com/photo-1514517220017-8ce97a34a7b6",
        image_public_id: "seed/steak-box/shabu-short-plate",
        current_price: "690.00",
        stock: 24,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000007",
        slug: "smash-burger-blend-8020",
        name: "Smash Burger Blend 80/20",
        description: "A burger grind designed for crisp edges, juicy centers, and high-volume service.",
        category_id: "30000000-0000-4000-8000-000000000003",
        image_url: "https://images.unsplash.com/photo-1568901346375-23c9450c58cd",
        image_public_id: "seed/steak-box/smash-burger-blend-8020",
        current_price: "420.00",
        stock: 30,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000008",
        slug: "oak-smoked-brisket-point",
        name: "Oak Smoked Brisket Point",
        description: "Heavier marbled point cut for low-and-slow smoke sessions and slicing boards.",
        category_id: "30000000-0000-4000-8000-000000000004",
        image_url: "https://images.unsplash.com/photo-1529193591184-b1d58069ecdd",
        image_public_id: "seed/steak-box/oak-smoked-brisket-point",
        current_price: "1590.00",
        stock: 6,
        is_active: true,
    },
    SeedProduct {
        id: "40000000-0000-4000-8000-000000000009",
        slug: "seasonal-hanger-steak",
        name: "Seasonal Hanger Steak",
        description: "A rotating butcher's cut reserved for special drops and admin merchandising tests.",
        category_id: "30000000-0000-4000-8000-000000000001",
        image_url: "https://images.unsplash.com/photo-1546964124-0cce460f38ef",
        image_public_id: "seed/steak-box/seasonal-hanger-steak",
        current_price: "1190.00",
        stock: 0,
        is_active: false,
    },
];

fn parse_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("seed UUID must be valid")
}

fn parse_money(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("seed money must be valid")
}

fn hash_password_sync(password: &str) -> SeedResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| io::Error::other(format!("password hashing failed: {error}")))?;
    Ok(hash.to_string())
}

async fn truncate_demo_tables(tx: &mut Transaction<'_, Postgres>) -> SeedResult {
    sqlx::query(
        r#"TRUNCATE TABLE
               pending_product_image_deletions,
               pending_product_images,
               product_images,
               product_slug_history,
               category_slug_history,
               order_items,
               orders,
               cart_items,
               carts,
               refresh_tokens,
               oauth_login_tickets,
               account_providers,
               email_verifications,
               addresses,
               products,
               categories,
               users
           RESTART IDENTITY CASCADE"#,
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_users(
    tx: &mut Transaction<'_, Postgres>,
    now: chrono::DateTime<Utc>,
) -> SeedResult {
    for user in DEMO_USERS {
        let hashed_password = hash_password_sync(user.password)?;

        sqlx::query(
            r#"INSERT INTO users (
                   id,
                   name,
                   email,
                   image,
                   image_public_id,
                   password_hash,
                   role,
                   is_active,
                   is_verified,
                   created_at,
                   updated_at
               )
               VALUES ($1, $2, $3, $4, NULL, $5, $6, TRUE, TRUE, $7, $7)"#,
        )
        .bind(parse_uuid(user.id))
        .bind(user.name)
        .bind(user.email)
        .bind(user.image)
        .bind(hashed_password)
        .bind(user.role)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn insert_addresses(
    tx: &mut Transaction<'_, Postgres>,
    now: chrono::DateTime<Utc>,
) -> SeedResult<HashMap<Uuid, AddressSnapshot>> {
    let mut snapshots = HashMap::new();

    for address in DEMO_ADDRESSES {
        let address_id = parse_uuid(address.id);
        let user_id = parse_uuid(address.user_id);

        sqlx::query(
            r#"INSERT INTO addresses (
                   id,
                   user_id,
                   recipient_name,
                   phone,
                   address_line,
                   city,
                   postal_code,
                   is_default,
                   created_at
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(address_id)
        .bind(user_id)
        .bind(address.recipient_name)
        .bind(address.phone)
        .bind(address.address_line)
        .bind(address.city)
        .bind(address.postal_code)
        .bind(address.is_default)
        .bind(now)
        .execute(&mut **tx)
        .await?;

        snapshots.insert(
            address_id,
            AddressSnapshot {
                recipient_name: address.recipient_name.to_string(),
                phone: address.phone.map(str::to_string),
                address_line: address.address_line.to_string(),
                city: address.city.to_string(),
                postal_code: address.postal_code.to_string(),
            },
        );
    }

    Ok(snapshots)
}

async fn insert_categories(
    tx: &mut Transaction<'_, Postgres>,
    now: chrono::DateTime<Utc>,
) -> SeedResult {
    for category in DEMO_CATEGORIES {
        let category_id = parse_uuid(category.id);

        sqlx::query(
            r#"INSERT INTO categories (id, slug, name, description, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $5)"#,
        )
        .bind(category_id)
        .bind(category.slug)
        .bind(category.name)
        .bind(category.description)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        r#"INSERT INTO category_slug_history (slug, category_id, created_at)
           VALUES
             ('chef-signatures', $1, $2),
             ('marbled-steaks', $3, $2)"#,
    )
    .bind(parse_uuid("30000000-0000-4000-8000-000000000001"))
    .bind(now - Duration::days(45))
    .bind(parse_uuid("30000000-0000-4000-8000-000000000002"))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_products(
    tx: &mut Transaction<'_, Postgres>,
    now: chrono::DateTime<Utc>,
) -> SeedResult<HashMap<Uuid, ProductSnapshot>> {
    let mut snapshots = HashMap::new();

    for (index, product) in DEMO_PRODUCTS.iter().enumerate() {
        let product_id = parse_uuid(product.id);
        let category_id = parse_uuid(product.category_id);
        let price = parse_money(product.current_price);
        let created_at = now - Duration::days((index as i64) + 2);

        sqlx::query(
            r#"INSERT INTO products (
                   id,
                   slug,
                   name,
                   description,
                   image_url,
                   image_public_id,
                   current_price,
                   stock,
                   is_active,
                   created_at,
                   updated_at,
                   category_id
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, $11)"#,
        )
        .bind(product_id)
        .bind(product.slug)
        .bind(product.name)
        .bind(product.description)
        .bind(product.image_url)
        .bind(product.image_public_id)
        .bind(price)
        .bind(product.stock)
        .bind(product.is_active)
        .bind(created_at)
        .bind(category_id)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"INSERT INTO product_images (
                   id,
                   product_id,
                   image_url,
                   image_public_id,
                   sort_order,
                   is_primary,
                   created_at
               )
               VALUES ($1, $2, $3, $4, 0, TRUE, $5)"#,
        )
        .bind(Uuid::now_v7())
        .bind(product_id)
        .bind(product.image_url)
        .bind(product.image_public_id)
        .bind(created_at)
        .execute(&mut **tx)
        .await?;

        snapshots.insert(
            product_id,
            ProductSnapshot {
                id: product_id,
                name: product.name.to_string(),
                price,
            },
        );
    }

    sqlx::query(
        r#"INSERT INTO product_slug_history (slug, product_id, created_at)
           VALUES
             ('wagyu-ribeye-reserve', $1, $2),
             ('dry-aged-striploin', $3, $2)"#,
    )
    .bind(parse_uuid("40000000-0000-4000-8000-000000000001"))
    .bind(now - Duration::days(30))
    .bind(parse_uuid("40000000-0000-4000-8000-000000000002"))
    .execute(&mut **tx)
    .await?;

    Ok(snapshots)
}

async fn insert_demo_cart(
    tx: &mut Transaction<'_, Postgres>,
    now: chrono::DateTime<Utc>,
) -> SeedResult {
    let cart_id = parse_uuid("50000000-0000-4000-8000-000000000001");
    let user_id = parse_uuid("10000000-0000-4000-8000-000000000002");

    sqlx::query(
        r#"INSERT INTO carts (id, user_id, created_at, updated_at)
           VALUES ($1, $2, $3, $3)"#,
    )
    .bind(cart_id)
    .bind(user_id)
    .bind(now - Duration::hours(2))
    .execute(&mut **tx)
    .await?;

    for (item_id, product_id, quantity) in [
        (
            "51000000-0000-4000-8000-000000000001",
            "40000000-0000-4000-8000-000000000001",
            1,
        ),
        (
            "51000000-0000-4000-8000-000000000002",
            "40000000-0000-4000-8000-000000000006",
            2,
        ),
    ] {
        sqlx::query(
            r#"INSERT INTO cart_items (id, cart_id, product_id, quantity, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $5)"#,
        )
        .bind(parse_uuid(item_id))
        .bind(cart_id)
        .bind(parse_uuid(product_id))
        .bind(quantity)
        .bind(now - Duration::minutes(90))
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn insert_order(
    tx: &mut Transaction<'_, Postgres>,
    product_map: &HashMap<Uuid, ProductSnapshot>,
    address_map: &HashMap<Uuid, AddressSnapshot>,
    input: SeedOrderInput,
) -> SeedResult {
    let order_id = parse_uuid(input.id);
    let user_id = parse_uuid(input.user_id);
    let address_id = parse_uuid(input.address_id);
    let address = address_map
        .get(&address_id)
        .cloned()
        .expect("seed address must exist");

    let mut total_amount = Decimal::ZERO;

    for item in &input.items {
        let product = product_map
            .get(&parse_uuid(item.product_id))
            .expect("seed product must exist");
        total_amount += product.price * Decimal::from(item.quantity);
    }

    sqlx::query(
        r#"INSERT INTO orders (
               id,
               user_id,
               shipping_address_id,
               shipping_recipient_name,
               shipping_phone,
               shipping_address_line,
               shipping_city,
               shipping_postal_code,
               total_amount,
               status,
               tracking_number,
               payment_slip_url,
               payment_slip_public_id,
               payment_submitted_at,
               created_at,
               updated_at
           )
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)"#,
    )
    .bind(order_id)
    .bind(user_id)
    .bind(address_id)
    .bind(&address.recipient_name)
    .bind(&address.phone)
    .bind(&address.address_line)
    .bind(&address.city)
    .bind(&address.postal_code)
    .bind(total_amount)
    .bind(input.status)
    .bind(input.tracking_number)
    .bind(input.payment_slip_url)
    .bind(input.payment_slip_public_id)
    .bind(input.payment_submitted_at)
    .bind(input.created_at)
    .bind(input.updated_at)
    .execute(&mut **tx)
    .await?;

    for item in &input.items {
        let product = product_map
            .get(&parse_uuid(item.product_id))
            .expect("seed product must exist");

        sqlx::query(
            r#"INSERT INTO order_items (
                   id,
                   order_id,
                   product_id,
                   product_name_at_purchase,
                   quantity,
                   price_at_purchase
               )
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(Uuid::now_v7())
        .bind(order_id)
        .bind(product.id)
        .bind(&product.name)
        .bind(item.quantity)
        .bind(product.price)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn insert_orders(
    tx: &mut Transaction<'_, Postgres>,
    product_map: &HashMap<Uuid, ProductSnapshot>,
    address_map: &HashMap<Uuid, AddressSnapshot>,
    now: chrono::DateTime<Utc>,
) -> SeedResult {
    let orders = vec![
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000001",
            user_id: "10000000-0000-4000-8000-000000000002",
            address_id: "20000000-0000-4000-8000-000000000001",
            status: "PAYMENT_REVIEW",
            tracking_number: None,
            payment_slip_url: Some(
                "https://res.cloudinary.com/demo/image/upload/v1710000000/steakbox/payment-review-001.jpg",
            ),
            payment_slip_public_id: Some("seed/steak-box/payment-review-001"),
            payment_submitted_at: Some(now - Duration::hours(34)),
            created_at: now - Duration::hours(36),
            updated_at: now - Duration::hours(34),
            items: vec![
                SeedOrderLine {
                    product_id: "40000000-0000-4000-8000-000000000001",
                    quantity: 1,
                },
                SeedOrderLine {
                    product_id: "40000000-0000-4000-8000-000000000006",
                    quantity: 2,
                },
            ],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000002",
            user_id: "10000000-0000-4000-8000-000000000003",
            address_id: "20000000-0000-4000-8000-000000000003",
            status: "PENDING",
            tracking_number: None,
            payment_slip_url: None,
            payment_slip_public_id: None,
            payment_submitted_at: None,
            created_at: now - Duration::hours(8),
            updated_at: now - Duration::hours(8),
            items: vec![SeedOrderLine {
                product_id: "40000000-0000-4000-8000-000000000007",
                quantity: 3,
            }],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000003",
            user_id: "10000000-0000-4000-8000-000000000003",
            address_id: "20000000-0000-4000-8000-000000000003",
            status: "PAYMENT_FAILED",
            tracking_number: None,
            payment_slip_url: Some(
                "https://res.cloudinary.com/demo/image/upload/v1710000000/steakbox/payment-failed-001.jpg",
            ),
            payment_slip_public_id: Some("seed/steak-box/payment-failed-001"),
            payment_submitted_at: Some(now - Duration::hours(20)),
            created_at: now - Duration::hours(26),
            updated_at: now - Duration::hours(10),
            items: vec![SeedOrderLine {
                product_id: "40000000-0000-4000-8000-000000000005",
                quantity: 1,
            }],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000004",
            user_id: "10000000-0000-4000-8000-000000000002",
            address_id: "20000000-0000-4000-8000-000000000002",
            status: "PAID",
            tracking_number: None,
            payment_slip_url: Some(
                "https://res.cloudinary.com/demo/image/upload/v1710000000/steakbox/paid-001.jpg",
            ),
            payment_slip_public_id: Some("seed/steak-box/paid-001"),
            payment_submitted_at: Some(now - Duration::days(2)),
            created_at: now - Duration::days(2) - Duration::hours(2),
            updated_at: now - Duration::days(1),
            items: vec![SeedOrderLine {
                product_id: "40000000-0000-4000-8000-000000000002",
                quantity: 1,
            }],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000005",
            user_id: "10000000-0000-4000-8000-000000000002",
            address_id: "20000000-0000-4000-8000-000000000001",
            status: "SHIPPED",
            tracking_number: Some("THPOST-RR458901235TH"),
            payment_slip_url: Some(
                "https://res.cloudinary.com/demo/image/upload/v1710000000/steakbox/shipped-001.jpg",
            ),
            payment_slip_public_id: Some("seed/steak-box/shipped-001"),
            payment_submitted_at: Some(now - Duration::days(5)),
            created_at: now - Duration::days(5) - Duration::hours(5),
            updated_at: now - Duration::days(3),
            items: vec![SeedOrderLine {
                product_id: "40000000-0000-4000-8000-000000000004",
                quantity: 2,
            }],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000006",
            user_id: "10000000-0000-4000-8000-000000000003",
            address_id: "20000000-0000-4000-8000-000000000003",
            status: "DELIVERED",
            tracking_number: Some("DHL-TH-778899001"),
            payment_slip_url: Some(
                "https://res.cloudinary.com/demo/image/upload/v1710000000/steakbox/delivered-001.jpg",
            ),
            payment_slip_public_id: Some("seed/steak-box/delivered-001"),
            payment_submitted_at: Some(now - Duration::days(8)),
            created_at: now - Duration::days(9),
            updated_at: now - Duration::days(6),
            items: vec![
                SeedOrderLine {
                    product_id: "40000000-0000-4000-8000-000000000003",
                    quantity: 1,
                },
                SeedOrderLine {
                    product_id: "40000000-0000-4000-8000-000000000007",
                    quantity: 2,
                },
            ],
        },
        SeedOrderInput {
            id: "60000000-0000-4000-8000-000000000007",
            user_id: "10000000-0000-4000-8000-000000000003",
            address_id: "20000000-0000-4000-8000-000000000003",
            status: "CANCELLED",
            tracking_number: None,
            payment_slip_url: None,
            payment_slip_public_id: None,
            payment_submitted_at: None,
            created_at: now - Duration::days(12),
            updated_at: now - Duration::days(11),
            items: vec![SeedOrderLine {
                product_id: "40000000-0000-4000-8000-000000000008",
                quantity: 1,
            }],
        },
    ];

    for order in orders {
        insert_order(tx, product_map, address_map, order).await?;
    }

    Ok(())
}

pub async fn sync_demo_product_assets(pool: &PgPool) -> SeedResult {
    let mut tx = pool.begin().await?;
    let now = Utc::now();

    for product in DEMO_PRODUCTS {
        let product_id = parse_uuid(product.id);

        sqlx::query(
            r#"UPDATE products
               SET image_url = $1,
                   image_public_id = $2,
                   updated_at = $3
               WHERE slug = $4"#,
        )
        .bind(product.image_url)
        .bind(product.image_public_id)
        .bind(now)
        .bind(product.slug)
        .execute(&mut *tx)
        .await?;

        let primary_result = sqlx::query(
            r#"UPDATE product_images
               SET image_url = $1,
                   image_public_id = $2
               WHERE product_id = $3
                 AND is_primary = TRUE"#,
        )
        .bind(product.image_url)
        .bind(product.image_public_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await?;

        if primary_result.rows_affected() == 0 {
            sqlx::query(
                r#"INSERT INTO product_images (
                       id,
                       product_id,
                       image_url,
                       image_public_id,
                       sort_order,
                       is_primary,
                       created_at
                   )
                   VALUES ($1, $2, $3, $4, 0, TRUE, $5)
                   ON CONFLICT (image_public_id) DO UPDATE
                   SET image_url = EXCLUDED.image_url,
                       product_id = EXCLUDED.product_id,
                       sort_order = EXCLUDED.sort_order,
                       is_primary = EXCLUDED.is_primary"#,
            )
            .bind(Uuid::now_v7())
            .bind(product_id)
            .bind(product.image_url)
            .bind(product.image_public_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    println!("Demo product assets synced.");

    Ok(())
}

pub async fn seed_demo_data(pool: &PgPool) -> SeedResult {
    let mut tx = pool.begin().await?;
    let now = Utc::now();

    truncate_demo_tables(&mut tx).await?;
    insert_users(&mut tx, now).await?;
    let address_map = insert_addresses(&mut tx, now).await?;
    insert_categories(&mut tx, now).await?;
    let product_map = insert_products(&mut tx, now).await?;
    insert_demo_cart(&mut tx, now).await?;
    insert_orders(&mut tx, &product_map, &address_map, now).await?;

    tx.commit().await?;

    println!("Demo storefront seed completed.");
    println!("Admin login   : admin@steakbox.dev / Admin12345!");
    println!("Customer login: chef@steakbox.dev / SteakBox123!");
    println!("Customer login: homecook@steakbox.dev / SteakBox123!");

    Ok(())
}
