# Rust E-Commerce Backend

REST API backend for an e-commerce platform built with Rust, Axum, and PostgreSQL. The project focuses on secure authentication, transactional order handling, and production-oriented operational basics such as health checks, cleanup jobs, and CI.

## Overview

- Email/password authentication with Argon2
- Email verification and email-change verification
- Google and GitHub login
- JWT access tokens and rotating refresh tokens via `HttpOnly` cookies
- Product management with admin-only image upload to Cloudinary
- Transactional order creation with stock locking
- Multiple user addresses with one-default-address enforcement
- Health/readiness endpoints and background cleanup jobs
- SQLx migrations and CI checks for `fmt`, `clippy`, and `test`

## Stack

- Rust (edition 2024)
- Axum
- SQLx + PostgreSQL
- Tokio
- Lettre
- Reqwest
- Cloudinary

## Project Layout

```text
backend-rust-2/
├── migrations/             # Database schema and hardening migrations
├── src/
│   ├── auth/               # Authentication and social login
│   ├── user/               # User profile and email change flow
│   ├── address/            # Shipping addresses
│   ├── product/            # Product and image flows
│   ├── order/              # Order placement
│   ├── shared/             # Common infrastructure and helpers
│   ├── bin/                # Local utility binaries
│   ├── config.rs           # Environment configuration
│   └── main.rs             # App bootstrap
├── API_DOCS.md             # Endpoint-level API reference
├── .env.example            # Local environment template
└── .github/workflows/ci.yml
```

## Quick Start

### 1. Prerequisites

- [Rust & Cargo](https://rustup.rs/)
- [PostgreSQL](https://www.postgresql.org/)
- [sqlx-cli](https://crates.io/crates/sqlx-cli)

Install `sqlx-cli`:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

If `sqlx` is not found after install:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### 2. Create Environment File

```bash
cp .env.example .env
```

Required integrations in `.env`:

- PostgreSQL
- JWT secret
- SMTP credentials
- Google/GitHub OAuth credentials
- Cloudinary credentials

Important runtime flags:

```bash
APP_ENV=development
LOG_JSON=false
TRUST_PROXY_HEADERS=false
CLEANUP_INTERVAL_MINUTES=10
PRODUCT_IMAGE_UPLOAD_TTL_MINUTES=60
COOKIE_SECURE=false
```

Production baseline:

```bash
APP_ENV=production
LOG_JSON=true
TRUST_PROXY_HEADERS=true
COOKIE_SECURE=true
APP_URL=https://your-frontend-domain
```

### 3. Create Database and Run Migrations

```bash
sqlx database create
sqlx migrate run
```

If the database already exists:

```bash
sqlx migrate run
```

### 4. Start the API

```bash
cargo run
```

Default local base URL:

```text
http://localhost:3000
```

Operational endpoints:

```text
GET /healthz
GET /readyz
```

## Useful Commands

Run quality checks:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Reset the local database from all migrations:

```bash
cargo run --bin reset_db
```

## Product Image Flow

Product images are uploaded to Cloudinary through the backend, not directly trusted from arbitrary URLs.

1. Admin uploads an image to `POST /api/products/upload-image`
2. Backend uploads the file to Cloudinary and returns `image_url` + `image_public_id`
3. Admin uses those values in `POST /api/products` or `PUT /api/products/{id}`

Current rules:

- Only `image/jpeg`, `image/png`, and `image/webp` are accepted
- File size is limited to `5 MB`
- Uploaded images must be attached to a product before `PRODUCT_IMAGE_UPLOAD_TTL_MINUTES`
- Replaced product images are deleted from Cloudinary
- Failed deletions are queued and retried by the cleanup worker

## Background Jobs

The app starts a cleanup worker on boot. It currently handles:

- expired refresh tokens
- expired email verification records
- expired pending product image uploads
- queued product image deletions

Email notifications are also dispatched asynchronously in detached app tasks for:

- verification email
- welcome email
- login notification
- email-change verification

## Security Notes

- Refresh tokens are stored as hashes, not raw tokens
- Email addresses are normalized before persistence
- Default address uniqueness is enforced at the database layer
- Auth-sensitive endpoints are rate limited in memory
- Proxy headers are only trusted when `TRUST_PROXY_HEADERS=true`
- Cookies use `HttpOnly; SameSite=Strict`, with `Secure` controlled by `COOKIE_SECURE`

## API Reference

Detailed endpoint documentation is in [API_DOCS.md](./API_DOCS.md).

## Deployment Notes

- Run migrations before serving traffic
- Keep app, PostgreSQL, and Redis/Valkey in the same region if you later move rate limiting out of memory
- Store secrets in platform secret management, not in committed files
- Set real Cloudinary credentials in production

## Status

The codebase currently passes:

- `cargo fmt`
- `cargo check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
