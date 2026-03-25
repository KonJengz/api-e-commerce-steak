# Deploying to a DigitalOcean Droplet

This guide deploys the API with:

- Docker Compose
- Caddy for HTTPS and reverse proxy
- Neon as the external PostgreSQL database
- Cloudinary for product images

Use this guide when this backend is the only public app on the Droplet. If you plan to host multiple backends on one Droplet, use [DEPLOY_DROPLET_MULTI_APP.md](/Users/thaweevitkittanmete/Desktop/konjeng/Developer/project-e-commerce-2026/backend-rust-2/DEPLOY_DROPLET_MULTI_APP.md) instead.

## Recommended Starting Point

- Droplet: Ubuntu 24.04 LTS
- Size: 1 GiB / 1 vCPU
- Region: Singapore if your users are in Thailand or nearby

## 1. Prepare DNS

Create an `A` record pointing your API subdomain to the Droplet IP:

- `api.example.com -> <droplet-ip>`

This subdomain should match `API_HOSTNAME` in `.env.droplet`.

## 2. Open Firewall Ports

Allow:

- `22/tcp` for SSH
- `80/tcp` for HTTP
- `443/tcp` for HTTPS

## 3. Install Docker and Compose on the Droplet

SSH into the Droplet and install Docker Engine plus the Compose plugin using Docker's official Ubuntu instructions.

After install, verify:

```bash
docker --version
docker compose version
```

## 4. Clone the Repository

```bash
git clone <your-repo-url>
cd backend-rust-2
```

## 5. Create the Production Environment File

```bash
cp .env.droplet.example .env.droplet
```

Fill in at least:

- `API_HOSTNAME`
- `ACME_EMAIL`
- `APP_URL`
- `DATABASE_URL_POOLED` or `DATABASE_URL`
- `JWT_SECRET`
- `SMTP_*`
- `CLOUDINARY_*`

Important:

- `APP_URL` is the frontend origin allowed by CORS, for example `https://app.example.com`
- `API_HOSTNAME` is the public backend domain served by Caddy, for example `api.example.com`
- If your frontend and backend use different top-level domains, refresh-token cookie behavior may not work as expected

## 6. Run Database Migrations

Run migrations against the production database before the first deploy and before any schema-dependent release:

```bash
sqlx migrate run
```

You can run this from your local machine as long as it points to the same Neon database.

## 7. Start the Stack

```bash
docker compose --env-file .env.droplet -f compose.droplet.yml up -d --build
```

This starts:

- `api`: the Rust backend container
- `caddy`: the public HTTPS entrypoint

## 8. Verify the Deployment

Check container status:

```bash
docker compose --env-file .env.droplet -f compose.droplet.yml ps
```

Follow logs:

```bash
docker compose --env-file .env.droplet -f compose.droplet.yml logs -f api
docker compose --env-file .env.droplet -f compose.droplet.yml logs -f caddy
```

Check health:

```bash
curl https://api.example.com/healthz
curl https://api.example.com/readyz
```

## 9. Updating the App

```bash
chmod +x scripts/deploy-droplet.sh
./scripts/deploy-droplet.sh
```

This script:

- runs `git fetch` + `git pull --ff-only`
- rebuilds and restarts the Docker Compose stack
- prints container status
- prints recent logs for the `api` service

Useful overrides:

```bash
BRANCH=main ./scripts/deploy-droplet.sh
LOG_SERVICE=caddy ./scripts/deploy-droplet.sh
SKIP_PULL=1 ./scripts/deploy-droplet.sh
```

## 10. Useful Operations

Restart:

```bash
docker compose --env-file .env.droplet -f compose.droplet.yml restart
```

Stop:

```bash
docker compose --env-file .env.droplet -f compose.droplet.yml down
```

Remove old images:

```bash
docker image prune -f
```

## Notes

- Caddy automatically provisions HTTPS certificates once DNS points at the Droplet and ports `80/443` are reachable.
- Background cleanup runs inside the API process, so it continues to work normally on a Droplet unlike scale-to-zero platforms.
- This setup keeps PostgreSQL outside the Droplet. If you later host multiple backends on the same Droplet, you can reuse the same Caddy instance and add more services behind it.
