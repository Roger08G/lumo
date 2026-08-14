#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "$script_dir/.." && pwd)"
env_file="$project_root/.env"
certificates_dir="$project_root/certs"
minimum_compose_version='2.24.4'
mode='deploy'
temporary_env=''
service_stopped=false
rollback_needed=false
rollback_available=false
backup_dir=''
old_image_id=''
old_data_volume=''
rollback_compose_files=''
proxy_mode=false
proxy_network_name=''
candidate_built=false
first_v2_cutover=false
previous_release_v2=false
previous_image_declares_v2=false
public_contract_error=''

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

read_env_value() {
    local name="$1"
    local line value

    line="$(grep -E "^${name}=" "$env_file" | tail -n 1 || true)"
    [[ -n "$line" ]] || return 1
    value="${line#*=}"
    value="${value%$'\r'}"
    if [[ "$value" == \"*\" && "$value" == *\" ]]; then
        value="${value:1:${#value}-2}"
    elif [[ "$value" == \'*\' && "$value" == *\' ]]; then
        value="${value:1:${#value}-2}"
    fi
    printf '%s' "$value"
}

resolve_project_name() {
    local name="${COMPOSE_PROJECT_NAME:-}"
    if [[ -z "$name" && -f "$env_file" ]]; then
        name="$(read_env_value COMPOSE_PROJECT_NAME || true)"
    fi
    if [[ -z "$name" ]]; then
        name='lumo'
    fi
    name="${name,,}"
    [[ "$name" =~ ^[a-z0-9][a-z0-9_-]*$ ]] \
        || fail 'set COMPOSE_PROJECT_NAME to a simple lowercase Docker Compose project name'
    printf '%s' "$name"
}

find_labeled_container() {
    local project_name="$1"
    local -a containers
    mapfile -t containers < <(
        docker ps --all --quiet \
            --filter "label=com.docker.compose.project=$project_name" \
            --filter 'label=com.docker.compose.service=lumo-api'
    )
    ((${#containers[@]} <= 1)) || fail 'multiple lumo-api containers exist for this Compose project'
    printf '%s' "${containers[0]:-}"
}

find_labeled_volume() {
    local project_name="$1"
    local -a volumes
    mapfile -t volumes < <(
        docker volume ls --quiet \
            --filter "label=com.docker.compose.project=$project_name" \
            --filter 'label=com.docker.compose.volume=lumo-data'
    )
    if ((${#volumes[@]} == 0)) && docker volume inspect "${project_name}_lumo-data" >/dev/null 2>&1; then
        volumes=("${project_name}_lumo-data")
    fi
    ((${#volumes[@]} <= 1)) || fail 'multiple lumo-data volumes exist for this Compose project'
    printf '%s' "${volumes[0]:-}"
}

version_at_least() {
    local actual="$1"
    local required="$2"
    local lowest
    lowest="$(printf '%s\n%s\n' "$required" "$actual" | LC_ALL=C sort -V | head -n 1)"
    [[ "$lowest" == "$required" ]]
}

check_public_v2_contract() {
    local connect_timeout="${1:-10}"
    local max_time="${2:-20}"
    local health_body legacy_status groups_status

    public_contract_error=''
    if ! health_body="$(
        curl --fail --silent --show-error \
            --connect-timeout "$connect_timeout" --max-time "$max_time" \
            "$public_url/health"
    )"; then
        public_contract_error='public health endpoint is unavailable'
        return 1
    fi
    if ! jq -e '.status == "ok" and .apiVersion == "v2"' \
        <<<"$health_body" >/dev/null; then
        public_contract_error='public health endpoint did not report Lumo API v2'
        return 1
    fi

    if ! legacy_status="$(
        curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
            --connect-timeout "$connect_timeout" --max-time "$max_time" \
            "$public_url/v1/state"
    )"; then
        public_contract_error='public legacy v1 route is unavailable'
        return 1
    fi
    if [[ "$legacy_status" != 410 ]]; then
        public_contract_error="public legacy v1 route returned $legacy_status instead of 410"
        return 1
    fi

    if ! groups_status="$(
        curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
            --connect-timeout "$connect_timeout" --max-time "$max_time" \
            "$public_url/v2/groups"
    )"; then
        public_contract_error='public GET /v2/groups is unavailable'
        return 1
    fi
    if [[ "$groups_status" != 405 ]]; then
        public_contract_error="public GET /v2/groups returned $groups_status instead of 405"
        return 1
    fi
}

check_guard_unchanged() {
    local connect_timeout="${1:-10}"
    local max_time="${2:-20}"
    local current_guard_hash

    [[ -n "$guard_hash" ]] || return 0
    current_guard_hash="$(
        curl --fail --silent --show-error --location \
            --connect-timeout "$connect_timeout" --max-time "$max_time" \
            "$LUMO_GUARD_URL" \
            | sha256sum | awk '{print $1}'
    )" || return 1
    [[ "$current_guard_hash" == "$guard_hash" ]]
}

rollback_release() {
    local rollback_container health
    local rollback_healthy=false

    printf 'Deployment failed after recreation; restoring the previous v2 image without rewinding live data.\n' >&2
    docker compose stop lumo-api >/dev/null 2>&1 || return 1
    docker image tag "$old_image_id" "lumo-api:rollback-${backup_dir##*/}" || return 1
    COMPOSE_FILE="$rollback_compose_files" \
        docker compose up -d --no-deps --force-recreate lumo-api || return 1

    for _ in {1..30}; do
        rollback_container="$(
            COMPOSE_FILE="$rollback_compose_files" docker compose ps --all -q lumo-api
        )"
        if [[ -n "$rollback_container" ]]; then
            health="$(
                docker inspect \
                    --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' \
                    "$rollback_container" 2>/dev/null || true
            )"
            case "$health" in
                healthy)
                    rollback_healthy=true
                    break
                    ;;
                unhealthy | exited | dead) break ;;
            esac
        fi
        sleep 2
    done

    if [[ "$rollback_healthy" == true ]]; then
        for _ in {1..6}; do
            if check_public_v2_contract 2 5 && check_guard_unchanged 2 5; then
                printf 'Previous lumo-api v2 image and environment restored with live data; backup retained in %s.\n' \
                    "$backup_dir" >&2
                return 0
            fi
            sleep 2
        done
    fi

    COMPOSE_FILE="$rollback_compose_files" \
        docker compose logs --tail=100 lumo-api >&2 || true
    [[ -z "$public_contract_error" ]] \
        || printf 'Rollback public contract failed: %s.\n' "$public_contract_error" >&2
    return 1
}

cleanup() {
    local status=$?
    trap - EXIT

    if ((status != 0)) && [[ "$rollback_needed" == true ]]; then
        if [[ "$rollback_available" == true && "$previous_release_v2" == true ]]; then
            if ! rollback_release; then
                COMPOSE_FILE="$rollback_compose_files" \
                    docker compose stop lumo-api >/dev/null 2>&1 || true
                printf 'CRITICAL: v2 rollback failed and lumo-api was stopped; live data and backup remain in place at %s.\n' \
                    "$backup_dir" >&2
            fi
        else
            docker compose stop lumo-api >/dev/null 2>&1 || true
            if [[ "$rollback_available" == true ]]; then
                printf 'CRITICAL: first v2 cutover failed; lumo-api was stopped and no v1 rollback was reported as healthy. Live data and backup remain in place at %s.\n' \
                    "$backup_dir" >&2
            else
                printf 'Deployment failed and lumo-api was stopped; no previous release was available.\n' >&2
            fi
        fi
    elif [[ "$service_stopped" == true ]]; then
        (cd "$project_root" && docker compose start lumo-api >/dev/null 2>&1) || true
    fi

    if [[ -n "$temporary_env" && -f "$temporary_env" ]]; then
        rm -f -- "$temporary_env"
    fi
    exit "$status"
}

usage() {
    cat <<'USAGE'
Usage: ./scripts/deploy.sh [--prepare-only | --first-v2-cutover]

  --prepare-only       Create .env and certs/ without building or starting Docker.
  --first-v2-cutover   Explicitly deploy over a release that does not already
                       satisfy the public v2-only contract. Automatic rollback
                       will stop fail-closed instead of restoring v1.
USAGE
}

trap cleanup EXIT

case "${1:-}" in
    '') ;;
    --prepare-only) mode='prepare' ;;
    --first-v2-cutover) first_v2_cutover=true ;;
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
    require_command docker
    require_command openssl
    docker info >/dev/null 2>&1 || fail 'Docker daemon is required to prove this is a pristine deployment'

    project_name="$(resolve_project_name)"
    detected_container="$(find_labeled_container "$project_name")"
    detected_volume="$(find_labeled_volume "$project_name")"
    detected_lumo_container="$(
        docker ps --all --quiet --filter 'label=com.docker.compose.service=lumo-api'
    )"
    detected_lumo_volume="$(
        docker volume ls --quiet --filter 'label=com.docker.compose.volume=lumo-data'
    )"
    if [[ -n "$detected_container" || -n "$detected_volume" \
        || -n "$detected_lumo_container" || -n "$detected_lumo_volume" ]]; then
        fail 'refusing to generate .env while an existing lumo-api container or lumo-data volume exists; restore the original .env/master key'
    fi

    port="${LUMO_API_PORT:-8443}"
    [[ "$port" =~ ^[0-9]+$ ]] && ((port >= 1 && port <= 65535)) \
        || fail 'LUMO_API_PORT must be between 1 and 65535'

    umask 077
    temporary_env="$(mktemp "$project_root/.env.tmp.XXXXXX")"
    printf 'COMPOSE_PROJECT_NAME=lumo\nLUMO_DATA_VOLUME=lumo_lumo-data\nLUMO_SERVER_MASTER_KEY=%s\nLUMO_ENABLE_LEGACY_V1=false\nLUMO_TRUST_PROXY_HEADERS=false\nLUMO_API_HOST=127.0.0.1\nLUMO_API_PORT=%s\n' \
        "$(openssl rand -hex 32)" "$port" >"$temporary_env"
    chmod 0600 "$temporary_env"
    mv -- "$temporary_env" "$env_file"
    temporary_env=''
    printf 'Created %s with a new random server master key.\n' "$env_file"
else
    chmod 0600 "$env_file"
fi

master_key_value="$(read_env_value LUMO_SERVER_MASTER_KEY || true)"
[[ ${#master_key_value} -ge 32 ]] \
    || fail 'LUMO_SERVER_MASTER_KEY must contain at least 32 characters'
[[ "$master_key_value" != *[[:space:]]* ]] \
    || fail 'LUMO_SERVER_MASTER_KEY cannot contain whitespace'
[[ "$master_key_value" != 'replace-with-a-long-random-server-secret' ]] \
    || fail 'replace the example server master key before deploying'
if [[ -v LUMO_SERVER_MASTER_KEY && "$LUMO_SERVER_MASTER_KEY" != "$master_key_value" ]]; then
    fail 'exported LUMO_SERVER_MASTER_KEY differs from the persistent value in .env'
fi

legacy_value="$(read_env_value LUMO_ENABLE_LEGACY_V1 || true)"
[[ "${legacy_value,,}" == false ]] \
    || fail 'LUMO_ENABLE_LEGACY_V1=false must be explicit in .env'
if [[ -v LUMO_ENABLE_LEGACY_V1 && "${LUMO_ENABLE_LEGACY_V1,,}" != false ]]; then
    fail 'LUMO_ENABLE_LEGACY_V1 must remain false during deployment'
fi

if [[ "$mode" == prepare ]]; then
    printf 'Environment prepared. Install fullchain.pem and privkey.pem in %s before deploying.\n' \
        "$certificates_dir"
    exit 0
fi

require_command curl
require_command docker
require_command jq
require_command openssl
require_command sha256sum
require_command sort
require_command stat
docker info >/dev/null 2>&1 || fail 'Docker daemon is not available'
docker compose version >/dev/null 2>&1 || fail 'Docker Compose v2 is required'
compose_version="$(docker compose version --short)"
compose_version="${compose_version#v}"
compose_version="${compose_version%%-*}"
version_at_least "$compose_version" "$minimum_compose_version" \
    || fail "Docker Compose $minimum_compose_version or newer is required (found $compose_version)"

public_url="${LUMO_PUBLIC_URL:-}"
if [[ -z "$public_url" ]]; then
    public_url="$(read_env_value LUMO_PUBLIC_URL || true)"
fi
[[ "$public_url" =~ ^https://[^[:space:]]+$ ]] \
    || fail 'LUMO_PUBLIC_URL with an https:// origin is required for deployment'
public_url="${public_url%/}"

certificate="$certificates_dir/fullchain.pem"
private_key="$certificates_dir/privkey.pem"
[[ -f "$certificate" ]] || fail "missing certificate: $certificate"
[[ -f "$private_key" ]] || fail "missing private key: $private_key"
[[ -r "$certificate" ]] || fail 'fullchain.pem must be readable by the deployment user'
[[ -r "$private_key" ]] || fail 'privkey.pem must be readable by the deployment user'

openssl x509 -in "$certificate" -noout >/dev/null 2>&1 \
    || fail 'fullchain.pem is not a valid PEM certificate'
openssl x509 -in "$certificate" -checkend 86400 -noout >/dev/null 2>&1 \
    || fail 'TLS certificate is expired or expires within 24 hours'
if [[ -n "${LUMO_TLS_HOSTNAME:-}" ]]; then
    openssl x509 -in "$certificate" -noout -checkhost "$LUMO_TLS_HOSTNAME" >/dev/null 2>&1 \
        || fail "fullchain.pem does not cover $LUMO_TLS_HOSTNAME"
fi
openssl pkey -in "$private_key" -noout -check >/dev/null 2>&1 \
    || fail 'privkey.pem is not a valid private key'

certificate_digest="$(
    openssl x509 -in "$certificate" -pubkey -noout \
        | openssl pkey -pubin -outform DER 2>/dev/null \
        | openssl dgst -sha256
)"
private_key_digest="$(
    openssl pkey -in "$private_key" -pubout -outform DER 2>/dev/null \
        | openssl dgst -sha256
)"
[[ "$certificate_digest" == "$private_key_digest" ]] \
    || fail 'TLS certificate and private key do not match'

for file in "$certificate" "$private_key"; do
    owner_id="$(stat -c '%u' "$file")"
    other_permissions="$(stat -c '%a' "$file")"
    other_permissions="${other_permissions: -1}"
    if [[ "$owner_id" != 10001 ]] && (((10#$other_permissions & 4) == 0)); then
        fail "$file is not readable by container UID 10001"
    fi
done

cd "$project_root"
docker compose config --quiet
compose_json="$(docker compose config --format json)"

jq -e '
    .services["lumo-api"].environment.LUMO_ENABLE_LEGACY_V1
    | tostring | ascii_downcase == "false"
' <<<"$compose_json" >/dev/null \
    || fail 'rendered Compose configuration must disable legacy v1'

if jq -e '
    (.services["lumo-api"].networks // {}) as $networks
    | if ($networks | type) == "object"
      then ($networks | has("reverse-proxy"))
      else ($networks | index("reverse-proxy") != null)
      end
' <<<"$compose_json" >/dev/null; then
    proxy_mode=true
    jq -e '((.services["lumo-api"].ports // []) | length) == 0' \
        <<<"$compose_json" >/dev/null \
        || fail 'proxy overlay must remove every published lumo-api port'
    proxy_network_name="$(jq -r '.networks["reverse-proxy"].name // empty' <<<"$compose_json")"
    [[ -n "$proxy_network_name" ]] || fail 'proxy overlay did not resolve its external network name'
    docker network inspect "$proxy_network_name" >/dev/null 2>&1 \
        || fail "external reverse proxy network does not exist: $proxy_network_name"
else
    jq -e '
        (.services["lumo-api"].ports // []) as $ports
        | ($ports | length) > 0
          and all($ports[]; .host_ip == "127.0.0.1" or .host_ip == "::1")
    ' <<<"$compose_json" >/dev/null \
        || fail 'lumo-api ports must be absent or bound exclusively to loopback'
fi

compose_project_name="$(jq -r '.name // empty' <<<"$compose_json")"
if [[ -z "$compose_project_name" ]]; then
    compose_project_name="$(resolve_project_name)"
fi
rendered_data_volume="$(jq -r '.volumes["lumo-data"].name // empty' <<<"$compose_json")"
[[ -n "$rendered_data_volume" ]] \
    || fail 'rendered Compose configuration did not resolve the lumo-data volume name'

mapfile -t labeled_lumo_containers < <(
    docker ps --all --quiet --filter 'label=com.docker.compose.service=lumo-api'
)
((${#labeled_lumo_containers[@]} <= 1)) \
    || fail 'multiple lumo-api Compose containers exist; reconcile them before deployment'
mapfile -t labeled_lumo_volumes < <(
    docker volume ls --quiet --filter 'label=com.docker.compose.volume=lumo-data'
)
((${#labeled_lumo_volumes[@]} <= 1)) \
    || fail 'multiple lumo-data Compose volumes exist; reconcile them before deployment'

if ((${#labeled_lumo_containers[@]} == 1)); then
    labeled_container="${labeled_lumo_containers[0]}"
    labeled_project="$(
        docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' \
            "$labeled_container"
    )"
    labeled_data_volume="$(
        docker inspect --format \
            '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}' \
            "$labeled_container"
    )"
    [[ "$labeled_project" == "$compose_project_name" ]] \
        || fail "existing lumo-api belongs to Compose project $labeled_project; set COMPOSE_PROJECT_NAME=$labeled_project before deploying"
    [[ -n "$labeled_data_volume" && "$labeled_data_volume" == "$rendered_data_volume" ]] \
        || fail "existing lumo-api uses data volume $labeled_data_volume; set LUMO_DATA_VOLUME=$labeled_data_volume before deploying"
fi
if ((${#labeled_lumo_volumes[@]} == 1)); then
    [[ "${labeled_lumo_volumes[0]}" == "$rendered_data_volume" ]] \
        || fail "existing lumo-data volume is ${labeled_lumo_volumes[0]}; set LUMO_DATA_VOLUME=${labeled_lumo_volumes[0]} before deploying"
fi

mapfile -t existing_containers < <(docker compose ps --all -q lumo-api)
((${#existing_containers[@]} <= 1)) || fail 'multiple lumo-api containers exist for this Compose project'
existing_container="${existing_containers[0]:-}"
existing_volume=''
if docker volume inspect "$rendered_data_volume" >/dev/null 2>&1; then
    existing_volume="$rendered_data_volume"
fi

if [[ -n "$existing_container" ]]; then
    running_master_key="$(
        docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$existing_container" \
            | sed -n 's/^LUMO_SERVER_MASTER_KEY=//p' \
            | tail -n 1
    )"
    if [[ -n "$running_master_key" && "$running_master_key" != "$master_key_value" ]]; then
        fail 'persistent LUMO_SERVER_MASTER_KEY differs from the existing container; refusing destructive recreation'
    fi
    if [[ -n "$running_master_key" ]]; then
        previous_image_declares_v2=true
    fi
    unset running_master_key
fi
unset master_key_value

if [[ "$previous_image_declares_v2" == true ]]; then
    [[ "$first_v2_cutover" == false ]] \
        || fail '--first-v2-cutover is only valid when the existing release is not v2'
    if ! check_public_v2_contract; then
        fail "existing v2 release does not satisfy the public rollback contract: $public_contract_error"
    fi
    previous_release_v2=true
elif [[ -n "$existing_container" ]]; then
    [[ "$first_v2_cutover" == true ]] \
        || fail 'existing release is not a validated public v2 release; follow the documented first cutover and pass --first-v2-cutover'
elif [[ -n "$existing_volume" ]]; then
    fail 'lumo-data exists without a container whose image and effective environment can be backed up; manual recovery is required'
elif [[ "$first_v2_cutover" == true ]]; then
    fail '--first-v2-cutover requires an existing lumo-api container or lumo-data volume'
fi

guard_hash=''
if [[ -n "${LUMO_GUARD_URL:-}" ]]; then
    guard_hash="$(
        curl --fail --silent --show-error --location \
            --connect-timeout 10 --max-time 20 \
            "$LUMO_GUARD_URL" \
            | sha256sum | awk '{print $1}'
    )"
fi

if [[ -n "$existing_container" || -n "$existing_volume" ]]; then
    release_stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    backup_dir="$project_root/backups/$release_stamp"
    mkdir -m 0700 "$backup_dir"
    mkdir -m 0700 "$backup_dir/data"
    install -m 0600 "$env_file" "$backup_dir/env.backup"
    git rev-parse HEAD >"$backup_dir/candidate-git-revision.txt"

    [[ -n "$existing_container" ]] \
        || fail 'a rollback backup requires the existing lumo-api container'
    old_image_id="$(docker inspect --format '{{.Image}}' "$existing_container")"
    data_mount_type="$(
        docker inspect --format \
            '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Type}}{{end}}{{end}}' \
            "$existing_container"
    )"
    old_data_volume="$(
        docker inspect --format \
            '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}' \
            "$existing_container"
    )"
    [[ "$data_mount_type" == volume && -n "$old_data_volume" ]] \
        || fail 'existing lumo-api /data mount is not the expected named volume'
    umask 077
    docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$existing_container" \
        | grep '^LUMO_' >"$backup_dir/previous-container.env"
    chmod 0600 "$backup_dir/previous-container.env"

    docker image tag "$old_image_id" "lumo-api:rollback-$release_stamp"
    printf '%s\n' "$old_image_id" >"$backup_dir/previous-image-id.txt"

    compose_file_setting="${COMPOSE_FILE:-}"
    if [[ -z "$compose_file_setting" ]]; then
        compose_file_setting="$(read_env_value COMPOSE_FILE || true)"
    fi
    compose_file_setting="${compose_file_setting:-docker-compose.yml}"
    compose_path_separator="${COMPOSE_PATH_SEPARATOR:-:}"
    rollback_compose_files="${compose_file_setting}${compose_path_separator}backups/${release_stamp}/rollback-compose.yml"
    printf '%s\n' \
        'services:' \
        '  lumo-api:' \
        '    build: !reset null' \
        "    image: lumo-api:rollback-$release_stamp" \
        '    environment: !reset []' \
        '    env_file:' \
        "      - ./backups/$release_stamp/previous-container.env" \
        >"$backup_dir/rollback-compose.yml"
    COMPOSE_FILE="$rollback_compose_files" docker compose config --quiet

    # Build while the previous release is still serving traffic. The coherent
    # data snapshot is taken only after the candidate image is ready.
    docker compose build --pull lumo-api
    candidate_built=true

    if [[ -n "$existing_container" ]] \
        && [[ "$(docker inspect --format '{{.State.Running}}' "$existing_container")" == true ]]; then
        docker compose stop lumo-api
        service_stopped=true
    fi

    deploy_uid="$(id -u)"
    deploy_gid="$(id -g)"
    docker run --rm \
        --network none \
        --read-only \
        --user 0 \
        --volume "$old_data_volume:/data:ro" \
        --volume "$backup_dir/data:/backup" \
        --entrypoint /bin/sh \
        "$old_image_id" \
        -c 'set -eu
            cp -a /data/. /backup/
            chown -R "$1:$2" /backup' \
        _ "$deploy_uid" "$deploy_gid"
    rollback_available=true
    printf 'Created rollback backup in %s.\n' "$backup_dir"
fi

if [[ "$candidate_built" != true ]]; then
    docker compose build --pull lumo-api
fi
rollback_needed=true
docker compose up -d --no-deps lumo-api
service_stopped=false

container_id="$(docker compose ps --all -q lumo-api)"
[[ -n "$container_id" ]] || fail 'lumo-api container was not created'

runtime_json="$(docker inspect "$container_id")"
if [[ "$proxy_mode" == true ]]; then
    jq -e '((.[0].HostConfig.PortBindings["8443/tcp"] // []) | length) == 0' \
        <<<"$runtime_json" >/dev/null \
        || fail 'running proxy-mode container unexpectedly publishes port 8443'
    jq -e --arg network "$proxy_network_name" \
        '.[0].NetworkSettings.Networks | has($network)' \
        <<<"$runtime_json" >/dev/null \
        || fail 'running lumo-api container is not attached to the reverse proxy network'
else
    jq -e '
        (.[0].HostConfig.PortBindings["8443/tcp"] // []) as $bindings
        | ($bindings | length) > 0
          and all($bindings[]; .HostIp == "127.0.0.1" or .HostIp == "::1")
    ' <<<"$runtime_json" >/dev/null \
        || fail 'running lumo-api port is not restricted to loopback'
fi

for _ in {1..30}; do
    health="$(
        docker inspect \
            --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' \
            "$container_id" 2>/dev/null || true
    )"
    case "$health" in
        healthy)
            check_public_v2_contract \
                || fail "$public_contract_error"
            check_guard_unchanged \
                || fail 'guard URL content changed during deployment'

            rollback_needed=false
            docker compose ps
            printf 'lumo-api v2 deployed successfully; legacy v1 is blocked.\n'
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
