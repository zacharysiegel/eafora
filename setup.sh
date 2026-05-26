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

function check_prerequisites {
    function required_program {
        local program_name="$1"
        local install_hint="$2"
        if ! which "${program_name}" 1>/dev/null 2>&1; then
            echo "The \`${program_name}\` program is required"
            echo "  install: ${install_hint}"
            exit 1
        fi
    }
    required_program "brew"   "https://brew.sh"
    required_program "cargo"  "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    required_program "dbmate" "brew install dbmate"
}
check_prerequisites

if ! test -f .env; then
    echo "Generating .env from template.env"
    cp template.env .env
fi

source ./.env

if ! brew ls --versions postgresql@18 >/dev/null 2>&1; then
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

echo "Building release ingestion binary (needed by the launchd job)"
cargo build --release -p ingestion

function install_launchd_ingestion_job {
    local launch_agents_dir="${HOME}/Library/LaunchAgents"
    local log_dir="${HOME}/Library/Logs/Eafora"
    local plist_path="${launch_agents_dir}/org.eafora.ingestion.plist"
    local ingestion_bin="${repo_dir}/target/release/ingestion"

    function render_plist {
        mkdir -p "${launch_agents_dir}" "${log_dir}"
        sed \
            -e "s|@@INGESTION_BIN@@|${ingestion_bin}|g" \
            -e "s|@@REPO_ROOT@@|${repo_dir}|g" \
            -e "s|@@LOG_DIR@@|${log_dir}|g" \
            ./scripts/eafora-ingestion.plist.template \
            > "${plist_path}"
    }

    function bootstrap_plist {
        if launchctl print "gui/$(id -u)/org.eafora.ingestion" >/dev/null 2>&1; then
            launchctl bootout "gui/$(id -u)/org.eafora.ingestion" || true
        fi
        launchctl bootstrap "gui/$(id -u)" "${plist_path}"
    }

    echo "Installing launchd job at ${plist_path} (Mondays 03:00)"
    render_plist
    bootstrap_plist
}
install_launchd_ingestion_job

echo "Setup complete."
