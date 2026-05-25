#!/bin/zsh
# Drops + recreates the eafora_test database and applies all migrations.
# Used by integration tests to start from a known-clean schema.

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"

source ./.env

test_database_url="${TEST_DATABASE_URL:-postgresql://localhost:5432/eafora_test}"

echo "Dropping eafora_test"
dropdb --if-exists eafora_test

echo "Creating eafora_test"
createdb eafora_test

echo "Applying migrations to eafora_test"
DBMATE_GLOBAL_OPTIONS=(--migrations-dir './ingestion/db/migrations' --no-dump-schema --wait --url "${test_database_url}?sslmode=disable")
dbmate "${DBMATE_GLOBAL_OPTIONS[@]}" up
