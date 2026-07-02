#!/bin/zsh

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"

source ./.env

DBMATE_GLOBAL_OPTIONS=(--migrations-dir './ingestion/db/migrations' --schema-file './ingestion/db/schema.sql' --wait --url "$DATABASE_URL?sslmode=disable")
dbmate "${DBMATE_GLOBAL_OPTIONS[@]}" "${@}"

echo "Regenerating sqlx caches"
cargo sqlx prepare --workspace
