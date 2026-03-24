# Rust E-Commerce Backend (Axum + SQLx)

A production-ready RESTful API backend for an e-commerce platform built with Rust. This project prioritizes security, performance, and best practices.

## 🚀 Key Features

* **Advanced Authentication & Authorization:**
  * traditional Email/Password login (Argon2 hashing).
  * 📧 **Email Verification** system via OTP.
  * 🔄 **Refresh Token Rotation** stored securely in **HttpOnly Cookies** (mitigates XSS).
  * 🌐 **Social Login** ready: Google & GitHub (OAuth2).
* **Robust E-Commerce Logic:**
  * 🛍️ Product catalog with stock management.
  * 💳 Transactional Orders (using PostgreSQL Transactions).
  * 📸 **Data Snapshots**: Snapshots product name and price at the time of purchase to prevent historical data corruption if prices change.
* **User Management:**
  * Profile management.
  * 🏠 **Multiple Shipping Addresses** with smart `is_default` handling.
* **Modern Tooling:**
  * **UUID v7** generated natively for time-sorted, collision-resistant primary keys.

## 🛠️ Tech Stack

* **Language**: [Rust](https://www.rust-lang.org/)
* **Web Framework**: [Axum](https://github.com/tokio-rs/axum)
* **Database**: PostgreSQL with [SQLx](https://github.com/launchbadge/sqlx) (async, compile-time query checking)
* **Authentication**: `jsonwebtoken` (JWT), `argon2` (hashing), `reqwest` (OAuth2 Verification)
* **Email Service**: `lettre` (SMTP)
* **Data Types**: `rust_decimal` (for precision currency), `uuid`, `chrono`

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
- [sqlx-cli](https://crates.io/crates/sqlx-cli): `cargo install sqlx-cli --no-default-features --features rustls,postgres`

### 2. Database Preparation
Create a new PostgreSQL database (e.g., `dnc01-workshop-ecom-rust`).

### 3. Environment Variables
Copy the example environment file and configure it:
```bash
cp .env.example .env
```
Update `.env` with your actual Postgres credentials, JWT secrets, SMTP credentials (e.g., Mailtrap), and OAuth client IDs.

### 4. Run Migrations
Initialize the database tables:
```bash
sqlx migrate run
```

### 5. Run the Server
Start the development server:
```bash
cargo run
```
The server will be available at `http://localhost:3000`.

## 📚 API Documentation

For detailed information on payloads, headers, and responses, please refer to the comprehensive [API Documentation](./API_DOCS.md).

## 🔒 Security Notes

- **Access Tokens** should be stored by the frontend (in memory or State Managers like Zustand/Redux).
- **Refresh Tokens** are automatically handled by the browser as `HttpOnly; Secure; SameSite=Strict` cookies to prevent XSS attacks.
- Frontend should utilize the **Token-based Flow** for Social Login, fetching the `id_token` (Google) or `code` (GitHub) and sending it to our backend, thereby eliminating complex server-side redirects.
