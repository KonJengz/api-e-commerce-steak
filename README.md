# Rust E-Commerce Backend (Axum + SQLx)

A production-ready RESTful API backend for an e-commerce platform built with Rust. This project prioritizes security, performance, and best practices.

## 🚀 Key Features

- **Advanced Authentication & Authorization**
- Traditional email/password login with Argon2 hashing
- Email verification via OTP
- Refresh token rotation with `HttpOnly` cookies
- Social login support for Google and GitHub
- **Robust E-Commerce Logic**
- Product catalog with stock management
- Transactional order creation with stock locking
- Product name/price snapshotting at purchase time
- **User Management**
- Profile lookup
- Multiple shipping addresses with one-default-address enforcement
- Email change verification flow
- **Modern Tooling**
- UUID v7 primary keys
- SQLx migrations
- CI for `fmt`, `clippy`, and `test`

## 🛠️ Tech Stack

- **Language**: [Rust](https://www.rust-lang.org/)
- **Web Framework**: [Axum](https://github.com/tokio-rs/axum)
- **Database**: PostgreSQL with [SQLx](https://github.com/launchbadge/sqlx)
- **Authentication**: `jsonwebtoken`, `argon2`, `reqwest`
- **Email Service**: `lettre`
- **Data Types**: `rust_decimal`, `uuid`, `chrono`

## 📂 Project Structure

```text
backend-rust-2/
├── migrations/             # SQLx database migration files
├── src/
│   ├── auth/               # Auth, Social Login, Token logic
│   ├── user/               # User profile endpoints
│   ├── address/            # Multiple addresses & default logic
│   ├── product/            # Product catalog endpoints
│   ├── order/              # Transactional order placement
│   ├── shared/             # Common utilities (jwt, password, email, errors)
│   ├── main.rs             # Application entry point & router mounting
│   └── config.rs           # Environment variable configurations
├── API_DOCS.md             # Detailed endpoint documentation
├── Cargo.toml              # Rust configurations & dependencies
└── .env.example            # Environment variable template
```

## ⚙️ Local Development Setup

### 1. Prerequisites
- [Rust & Cargo](https://rustup.rs/)
- [PostgreSQL](https://www.postgresql.org/)
- [sqlx-cli](https://crates.io/crates/sqlx-cli)

Install `sqlx-cli`:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

If `sqlx` is still not found after install, add Cargo's bin directory to your shell:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### 2. Database Preparation
Create a new PostgreSQL database (e.g., `dnc01-workshop-ecom-rust`).

### 3. Environment Variables
Copy the example environment file and configure it:
```bash
cp .env.example .env
```
Update `.env` with your actual Postgres credentials, JWT secrets, SMTP credentials, and OAuth client IDs.

Important runtime flags:

```bash
APP_ENV=development
LOG_JSON=false
TRUST_PROXY_HEADERS=false
CLEANUP_INTERVAL_MINUTES=10
PRODUCT_IMAGE_UPLOAD_TTL_MINUTES=60
COOKIE_SECURE=false
```

For production:

```bash
APP_ENV=production
LOG_JSON=true
TRUST_PROXY_HEADERS=true
COOKIE_SECURE=true
APP_URL=https://your-frontend-domain
```

### 4. Run Migrations
Initialize the database tables:
```bash
sqlx migrate run
```

If the database does not exist yet:

```bash
sqlx database create
sqlx migrate run
```

### 5. Run the Server
Start the development server:
```bash
cargo run
```
The server will be available at `http://localhost:3000`.

Health endpoints:

```bash
GET /healthz
GET /readyz
```

### 6. Quality Checks
Run the same checks as CI locally:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Optional local database reset utility:

```bash
cargo run --bin reset_db
```

## 📚 API Documentation

For detailed information on payloads, headers, and responses, please refer to the comprehensive [API Documentation](./API_DOCS.md).

## 🔒 Security Notes

- **Access Tokens** should be stored by the frontend (in memory or State Managers like Zustand/Redux).
- **Refresh Tokens** are automatically handled by the browser as `HttpOnly; SameSite=Strict` cookies, with `Secure` controlled by `COOKIE_SECURE`.
- Frontend should utilize the **Token-based Flow** for Social Login, fetching the `id_token` (Google) or `code` (GitHub) and sending it to our backend.
- Email addresses are normalized before persistence, and the database enforces the normalized form.
- Default address uniqueness and verification-record uniqueness are enforced at the database layer via migrations.
- Expired refresh tokens and verification records are cleaned up in the background on a fixed interval.
- Pending product image uploads are cleaned up in the background if they are uploaded but never attached to a product before `PRODUCT_IMAGE_UPLOAD_TTL_MINUTES`.
- Proxy headers are only trusted when `TRUST_PROXY_HEADERS=true`.
