#!/bin/zsh
# One-shot bootstrap for a fresh dev machine. Idempotent — safe to re-run.
#   - generates .env from template.env if missing
#   - installs Postgres 18 via Homebrew if missing, starts it as a launchd service
#   - creates the eafora database (no-op if it already exists)
#   - applies migrations to eafora via dbmate.sh
#   - applies migrations to eafora_test via scripts/setup-test-db.sh

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"

if ! test -f .env; then
    echo "Generating .env from template.env"
    cp template.env .env
fi

source ./.env

if ! brew list --formula 2>/dev/null | grep -q '^postgresql@18$'; then
    echo "Installing postgresql@18 via Homebrew"
    brew install postgresql@18
fi

postgres_bin="/opt/homebrew/opt/postgresql@18/bin"
export PATH="${postgres_bin}:${PATH}"

postgres_port=$(echo "${DATABASE_URL}" | sed -E 's|^postgresql://[^:/]+:([0-9]+)/.*$|\1|')
postgresql_conf="/opt/homebrew/var/postgresql@18/postgresql.conf"
if ! grep -q "^port = ${postgres_port}\b" "${postgresql_conf}"; then
    echo "Configuring postgresql@18 to listen on port ${postgres_port}"
    sed -i '' -E "s|^#?port = [0-9]+.*|port = ${postgres_port}|" "${postgresql_conf}"
    if brew services list | grep -q 'postgresql@18.*started'; then
        brew services restart postgresql@18
    else
        brew services start postgresql@18
    fi
    sleep 2
elif ! brew services list | grep -q 'postgresql@18.*started'; then
    echo "Starting postgresql@18 service"
    brew services start postgresql@18
    sleep 2
fi

if ! psql -p "${postgres_port}" -lqt | cut -d '|' -f 1 | grep -qw eafora; then
    echo "Creating eafora database"
    createdb -p "${postgres_port}" eafora
fi

echo "Applying migrations to eafora"
./dbmate.sh up

echo "Applying migrations to eafora_test"
./scripts/setup-test-db.sh

echo "Setup complete."
