#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REMOTE="${REMOTE:-origin}"
BRANCH="${BRANCH:-main}"
COMPOSE_FILE="${COMPOSE_FILE:-compose.droplet.yml}"
ENV_FILE="${ENV_FILE:-.env.droplet}"
LOG_SERVICE="${LOG_SERVICE:-api}"
LOG_TAIL="${LOG_TAIL:-50}"
SKIP_PULL="${SKIP_PULL:-0}"

if [[ ! -f "$COMPOSE_FILE" ]]; then
  echo "Compose file not found: $COMPOSE_FILE" >&2
  exit 1
fi

if [[ ! -f "$ENV_FILE" ]]; then
  echo "Env file not found: $ENV_FILE" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Refusing to deploy with tracked local changes." >&2
  echo "Commit, stash, or revert tracked edits on the Droplet first." >&2
  git status --short
  exit 1
fi

if [[ "$SKIP_PULL" != "1" ]]; then
  echo "==> Updating repository from $REMOTE/$BRANCH"
  git fetch "$REMOTE" "$BRANCH"
  git checkout "$BRANCH"
  git pull --ff-only "$REMOTE" "$BRANCH"
fi

echo "==> Deploying with $COMPOSE_FILE"
docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE" up -d --build --remove-orphans

echo "==> Container status"
docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE" ps

echo "==> Recent logs: $LOG_SERVICE"
docker compose --env-file "$ENV_FILE" -f "$COMPOSE_FILE" logs --tail="$LOG_TAIL" "$LOG_SERVICE"
