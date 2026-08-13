#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "$script_dir/.." && pwd)"
env_file="$project_root/.env"
certificates_dir="$project_root/certs"
mode='deploy'
temporary_env=''

cleanup() {
    if [[ -n "$temporary_env" && -f "$temporary_env" ]]; then
        rm -f -- "$temporary_env"
    fi
}
trap cleanup EXIT

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

usage() {
    cat <<'USAGE'
Usage: ./scripts/deploy.sh [--prepare-only]

  --prepare-only  Create .env and certs/ without building or starting Docker.
USAGE
}

case "${1:-}" in
    '') ;;
    --prepare-only) mode='prepare' ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

if [[ $# -gt 1 ]]; then
    usage >&2
    exit 2
fi

mkdir -p -- "$certificates_dir"
chmod 0755 "$certificates_dir"

if [[ ! -f "$env_file" ]]; then
    require_command openssl
    port="${LUMO_API_PORT:-8443}"
    [[ "$port" =~ ^[0-9]+$ ]] && ((port >= 1 && port <= 65535)) || fail 'LUMO_API_PORT must be between 1 and 65535'

    umask 077
    temporary_env="$(mktemp "$project_root/.env.tmp.XXXXXX")"
    printf 'LUMO_API_PASSWORD=%s\nLUMO_API_PORT=%s\n' "$(openssl rand -hex 32)" "$port" >"$temporary_env"
    chmod 0600 "$temporary_env"
    mv -- "$temporary_env" "$env_file"
    temporary_env=''
    printf 'Created %s with a new random secret.\n' "$env_file"
else
    chmod 0600 "$env_file"
fi

password_line="$(grep -E '^LUMO_API_PASSWORD=' "$env_file" | tail -n 1 || true)"
password_value="${password_line#LUMO_API_PASSWORD=}"
password_value="${password_value%$'\r'}"
if [[ "$password_value" == \"*\" && "$password_value" == *\" ]]; then
    password_value="${password_value:1:${#password_value}-2}"
fi
[[ ${#password_value} -ge 32 ]] || fail 'LUMO_API_PASSWORD must contain at least 32 characters'
[[ "$password_value" != *[[:space:]]* ]] || fail 'LUMO_API_PASSWORD cannot contain whitespace'
[[ "$password_value" != 'replace-with-a-long-random-secret' ]] || fail 'replace the example API password before deploying'

if [[ "$mode" == 'prepare' ]]; then
    printf 'Environment prepared. Install fullchain.pem and privkey.pem in %s before deploying.\n' "$certificates_dir"
    exit 0
fi

require_command docker
require_command openssl
require_command stat
docker compose version >/dev/null 2>&1 || fail 'Docker Compose v2 is required'
docker info >/dev/null 2>&1 || fail 'Docker daemon is not available'

certificate="$certificates_dir/fullchain.pem"
private_key="$certificates_dir/privkey.pem"
[[ -f "$certificate" ]] || fail "missing certificate: $certificate"
[[ -f "$private_key" ]] || fail "missing private key: $private_key"

openssl x509 -in "$certificate" -noout >/dev/null 2>&1 || fail 'fullchain.pem is not a valid PEM certificate'
openssl x509 -in "$certificate" -checkend 86400 -noout >/dev/null 2>&1 || fail 'TLS certificate is expired or expires within 24 hours'
openssl pkey -in "$private_key" -noout -check >/dev/null 2>&1 || fail 'privkey.pem is not a valid private key'

certificate_digest="$(openssl x509 -in "$certificate" -pubkey -noout | openssl pkey -pubin -outform DER 2>/dev/null | openssl dgst -sha256)"
private_key_digest="$(openssl pkey -in "$private_key" -pubout -outform DER 2>/dev/null | openssl dgst -sha256)"
[[ "$certificate_digest" == "$private_key_digest" ]] || fail 'TLS certificate and private key do not match'

for file in "$certificate" "$private_key"; do
    owner_id="$(stat -c '%u' "$file")"
    other_permissions="$(stat -c '%a' "$file")"
    other_permissions="${other_permissions: -1}"
    if [[ "$owner_id" != '10001' ]] && (((10#$other_permissions & 4) == 0)); then
        fail "$file is not readable by container UID 10001"
    fi
done

cd "$project_root"
docker compose config --quiet
docker compose build --pull
docker compose up -d --remove-orphans

container_id="$(docker compose ps -q lumo-api)"
[[ -n "$container_id" ]] || fail 'lumo-api container was not created'

for _ in {1..30}; do
    health="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$container_id" 2>/dev/null || true)"
    case "$health" in
        healthy)
            docker compose ps
            printf 'lumo-api deployed successfully.\n'
            exit 0
            ;;
        unhealthy | exited | dead)
            docker compose logs --tail=100 lumo-api >&2 || true
            fail "lumo-api entered state: $health"
            ;;
    esac
    sleep 2
done

docker compose logs --tail=100 lumo-api >&2 || true
fail 'lumo-api did not become healthy within 60 seconds'
