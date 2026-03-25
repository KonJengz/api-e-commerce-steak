# Multi-App Deployment on One DigitalOcean Droplet

This guide is for running multiple backend services on a single Droplet while sharing one public Caddy reverse proxy.

It uses:

- one shared Docker network: `edge`
- one shared Caddy service bound to ports `80/443`
- one Docker Compose stack per backend service

## Recommended Layout

Example on the Droplet:

```text
/opt/apps/
├── proxy/
│   ├── compose.droplet.proxy.yml
│   ├── .env.proxy
│   └── deploy/proxy/sites/
├── backend-rust-2/
│   ├── compose.droplet.app.yml
│   └── .env.droplet
└── another-backend/
    ├── compose.droplet.app.yml
    └── .env.droplet
```

## 1. Create the Shared Docker Network

Run once on the Droplet:

```bash
docker network create edge
```

## 2. Start the Shared Caddy Proxy

In the proxy directory:

```bash
cp .env.proxy.example .env.proxy
```

Set:

- `ACME_EMAIL`

Create one site file per backend in `deploy/proxy/sites/`.

For this repository, use the provided example and edit the domain:

```bash
cp deploy/proxy/sites/backend-rust-2-api.example.caddy deploy/proxy/sites/backend-rust-2-api.caddy
```

Example site:

```caddy
api.example.com {
    encode zstd gzip
    header {
        -Server
    }
    reverse_proxy backend-rust-2-api:8000
}
```

Start the proxy:

```bash
docker compose --env-file .env.proxy -f compose.droplet.proxy.yml up -d
```

## 3. Start This Backend as an App-Only Stack

In this repository:

```bash
cp .env.droplet.example .env.droplet
```

Fill in production values.

Important:

- `SERVICE_NAME` is not stored in `.env.droplet`; pass it at runtime if you want to override the default alias
- default alias for this app is `backend-rust-2-api`

Start the app-only stack:

```bash
docker compose --env-file .env.droplet -f compose.droplet.app.yml up -d --build
```

If you want a custom upstream alias:

```bash
SERVICE_NAME=my-shop-api docker compose --env-file .env.droplet -f compose.droplet.app.yml up -d --build
```

Then point the Caddy site file at `my-shop-api:8000`.

## 4. Add Another Backend Later

For each additional backend:

1. Clone the repo into its own folder
2. Connect the service to the same `edge` network
3. Give it a unique alias, for example `orders-api`
4. Add a new Caddy site file, for example:

```caddy
orders.example.com {
    encode zstd gzip
    reverse_proxy orders-api:8000
}
```

5. Reload the proxy:

```bash
docker compose --env-file .env.proxy -f compose.droplet.proxy.yml up -d
```

## 5. Notes

- Only one proxy stack should bind `80/443` on the Droplet
- Each backend stack should use `compose.droplet.app.yml`, not the all-in-one `compose.droplet.yml`
- This keeps TLS, domains, and public ingress in one place while each backend remains separately deployable
