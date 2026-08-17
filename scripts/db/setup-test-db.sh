#!/bin/zsh
# Drops + recreates the eafora_test database and applies all migrations.
# Used by integration tests to start from a known-clean schema.

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"

source ./.env

postgres_port=$(echo "${DATABASE_URL}" | sed -E 's|^postgresql://[^:/]+:([0-9]+)/.*$|\1|')
test_database_url="${TEST_DATABASE_URL:-postgresql://localhost:${postgres_port}/eafora_test}"

echo "Dropping eafora_test"
dropdb --if-exists -p "${postgres_port}" eafora_test

echo "Creating eafora_test"
createdb -p "${postgres_port}" eafora_test

echo "Applying migrations to eafora_test"
DBMATE_GLOBAL_OPTIONS=(--migrations-dir './ingestion/db/migrations' --no-dump-schema --wait --url "${test_database_url}?sslmode=disable")
dbmate "${DBMATE_GLOBAL_OPTIONS[@]}" up
