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
- Resend
- Reqwest
- Cloudinary

## Project Layout

```text
backend-rust-2/
├── deploy/                 # Reverse proxy and deployment helper files
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
├── DEPLOY_DROPLET.md       # Step-by-step Droplet deployment guide
├── DEPLOY_DROPLET_MULTI_APP.md # Shared-proxy guide for multiple backends
├── .env.example            # Local environment template
├── .env.droplet.example    # Production template for Docker Compose on a Droplet
├── .env.proxy.example      # Shared proxy template for multi-app Droplets
├── compose.droplet.app.yml # App-only stack for shared-proxy Droplets
├── compose.droplet.yml     # Production Docker Compose stack
├── compose.droplet.proxy.yml # Shared Caddy stack for multi-app Droplets
├── Dockerfile              # Container image for Koyeb or self-hosting
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
- Resend API key and verified sender address
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
RESEND_API_KEY=re_...
EMAIL_FROM=noreply@yourdomain.com
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

- order confirmation

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

### Koyeb

This repository can be deployed to Koyeb using the included [Dockerfile](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/Dockerfile).

Suggested Koyeb setup:

- Deploy from GitHub
- Builder: `Dockerfile`
- Exposed HTTP port: `8000`
- Health check path: `/readyz`

Important environment values:

- `APP_ENV=production`
- `LOG_JSON=true`
- `TRUST_PROXY_HEADERS=true`
- `COOKIE_SECURE=true`
- `APP_URL=https://your-frontend-domain`
- `DATABASE_URL_POOLED` or `DATABASE_URL`
- `JWT_SECRET`
- `RESEND_API_KEY`
- `EMAIL_FROM`
- `CLOUDINARY_CLOUD_NAME`
- `CLOUDINARY_API_KEY`
- `CLOUDINARY_API_SECRET`

Notes:

- Koyeb free services can scale to zero, so background cleanup only runs while the web service is awake.
- Run `sqlx migrate run` against your production database before first deploy or before enabling new schema-dependent features.

### DigitalOcean Droplet

This repository includes a production-oriented Docker Compose stack with Caddy for self-managed deployment on a Droplet.

Files:

- [compose.droplet.yml](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/compose.droplet.yml)
- [deploy/Caddyfile](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/deploy/Caddyfile)
- [.env.droplet.example](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/.env.droplet.example)
- [DEPLOY_DROPLET.md](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/DEPLOY_DROPLET.md)
- [compose.droplet.app.yml](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/compose.droplet.app.yml)
- [compose.droplet.proxy.yml](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/compose.droplet.proxy.yml)
- [.env.proxy.example](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/.env.proxy.example)
- [deploy/proxy/Caddyfile](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/deploy/proxy/Caddyfile)
- [DEPLOY_DROPLET_MULTI_APP.md](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/DEPLOY_DROPLET_MULTI_APP.md)

Quick start:

```bash
cp .env.droplet.example .env.droplet
docker compose --env-file .env.droplet -f compose.droplet.yml up -d --build
```

Use a real domain in `API_HOSTNAME` and run migrations before the first deploy.

After the first deploy on a Droplet, updates can be applied with one command:

```bash
chmod +x scripts/deploy-droplet.sh
./scripts/deploy-droplet.sh
```

For the shared-proxy pattern, use:

```bash
COMPOSE_FILE=compose.droplet.app.yml ./scripts/deploy-droplet.sh
```

If you want to host multiple backends on one Droplet, use the shared-proxy pattern in [DEPLOY_DROPLET_MULTI_APP.md](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/DEPLOY_DROPLET_MULTI_APP.md) instead of binding `80/443` in every app stack.

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
- `RESEND_API_KEY`
- `EMAIL_FROM`
- `CLOUDINARY_CLOUD_NAME`
- `CLOUDINARY_API_KEY`
- `CLOUDINARY_API_SECRET`

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
