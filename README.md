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

Database URL behavior:

- `DATABASE_URL`: default URL used by local development and SQLx CLI
- `DATABASE_URL_POOLED`: optional runtime URL, preferred by the app if present
- `DATABASE_URL_DIRECT`: optional direct URL for admin tools such as `reset_db`

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

### Neon Setup

If you want to try this project on Neon:

1. Create a Neon project
2. Copy the direct connection string from Neon
3. Put the direct URL in `DATABASE_URL`
4. Optionally put Neon pooled URL in `DATABASE_URL_POOLED`

Example:

```env
DATABASE_URL=postgresql://user:password@ep-example.us-east-2.aws.neon.tech/neondb?sslmode=require
DATABASE_URL_DIRECT=postgresql://user:password@ep-example.us-east-2.aws.neon.tech/neondb?sslmode=require
DATABASE_URL_POOLED=postgresql://user:password@ep-example-pooler.us-east-2.aws.neon.tech/neondb?sslmode=require
```

Recommended usage with Neon:

- use `DATABASE_URL` or `DATABASE_URL_DIRECT` for `sqlx migrate run`
- use `DATABASE_URL_POOLED` for the running API if you want pooled runtime connections
- keep migrations and schema tools on the direct URL

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

### DigitalOcean App Platform

Recommended for this repository if you want a managed long-running Rust service without rewriting the app for serverless.

Suggested setup:

- Service type: Web Service
- Source: GitHub repo
- Build method: Native Rust buildpack
- Build command: `cargo build --release`
- Run command: `./target/release/backend-rust-2`
- Health check path: `/readyz`

Important environment values:

- `APP_ENV=production`
- `LOG_JSON=true`
- `TRUST_PROXY_HEADERS=true`
- `COOKIE_SECURE=true`
- `APP_URL=https://your-frontend-domain`
- `DATABASE_URL_POOLED` or `DATABASE_URL` from Neon
- `JWT_SECRET`
- `CLOUDINARY_CLOUD_NAME`
- `CLOUDINARY_API_KEY`
- `CLOUDINARY_API_SECRET`
- `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM`

Notes:

- The app now accepts `PORT` automatically, so you do not need to set `APP_PORT` on App Platform unless you want to override it.
- Run `sqlx migrate run` against your production database before first deploy or before enabling new schema-dependent features.
- If you later want cleanup tasks to run independently of web traffic, move them into an App Platform scheduled job.

## Status

The codebase currently passes:

- `cargo fmt`
- `cargo check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
