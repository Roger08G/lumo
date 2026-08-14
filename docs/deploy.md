# Despliegue de `lumo-api`

## Requisitos

- Linux, OpenSSL, `jq`, Docker Engine y Docker Compose `2.24.4` o posterior.
- Dominio HTTPS y reloj sincronizado por NTP.
- Certificado y clave PEM disponibles para el contenedor.
- Nginx u otro proxy que preserve método, ruta, cuerpo y cabeceras `Authorization` y `X-Lumo-*`.

## Configuración del servidor

Archivos no versionados:

```text
.env
certs/fullchain.pem
certs/privkey.pem
```

En una instalación nueva deja que el script genere `.env` sin reutilizar ninguna credencial
antigua del cliente:

```bash
./scripts/deploy.sh --prepare-only
```

El script se niega a generar una clave si detecta un contenedor `lumo-api` o un volumen
`lumo-data`, incluso si están parados. En ese caso recupera el `.env` original o una copia cifrada;
no continúes con una clave nueva. `LUMO_ENABLE_LEGACY_V1=false` debe estar declarado de forma
explícita.

La identidad Docker también es persistente. Una instalación nueva usa estos valores, que no deben
cambiar al mover o renombrar el checkout:

```dotenv
COMPOSE_PROJECT_NAME=lumo
LUMO_DATA_VOLUME=lumo_lumo-data
```

Antes de recrear nada, el script compara ambos valores con los labels y el mount `/data` del
contenedor vivo. Si el VPS histórico usa otros nombres, configura en `.env` exactamente los que
muestre `docker inspect`; el script falla en vez de crear un segundo volumen o un segundo alias
`lumo-api` en la red de Nginx.

`LUMO_SERVER_MASTER_KEY` cifra las claves de estado en SQLite y nunca se compila en la APK. Debe
guardarse junto con las copias de seguridad. La base conserva un verificador no reversible y la
API se niega a arrancar si la clave configurada no coincide. No borres ni regeneres `.env` mientras
exista un volumen de datos. Variables opcionales:

```dotenv
LUMO_MAX_GROUPS=1000
LUMO_MAX_DEVICES_PER_GROUP=8
LUMO_MAX_ACTIVE_INVITES_PER_GROUP=8
LUMO_BOOTSTRAP_PER_IP=5
LUMO_BOOTSTRAP_GLOBAL=100
LUMO_BOOTSTRAP_WINDOW_SECONDS=3600
LUMO_INVITE_TTL_SECONDS=600
```

Activa `LUMO_TRUST_PROXY_HEADERS=true` sólo si la API no es accesible directamente y el proxy
sobrescribe la IP recibida; de lo contrario un cliente podría falsear la clave del rate limit.
Cuando Cloudflare esté delante, configura `real_ip_header CF-Connecting-IP` exclusivamente para
sus rangos oficiales y deja que Nginx escriba `X-Real-IP`; no reenvíes un `X-Real-IP` aportado por
el cliente.

Instala los certificados de forma que el UID `10001` pueda leerlos:

```bash
mkdir -p certs
deploy_group="$(id -g)"
sudo install -o 10001 -g "$deploy_group" -m 0444 /ruta/fullchain.pem certs/fullchain.pem
sudo install -o 10001 -g "$deploy_group" -m 0440 /ruta/privkey.pem certs/privkey.pem
```

## Proxy Docker

Si Nginx comparte una red Docker con la API:

```dotenv
COMPOSE_FILE=docker-compose.yml:docker-compose.proxy.yml
LUMO_PROXY_NETWORK=nombre_de_la_red_nginx
LUMO_TRUST_PROXY_HEADERS=true
```

El Compose base ya aplica filesystem de sólo lectura salvo `/data`, elimina capabilities, impide
elevar privilegios y limita CPU, memoria, procesos y logs. El overlay sólo elimina el puerto
loopback y conecta la API a la red externa del proxy. El proxy debe aceptar `/health` y `/v2/`;
`/v1/` debe quedar bloqueado. Configuración mínima:

```nginx
# Docker DNS. La variable obliga a Nginx a resolver de nuevo el contenedor recreado.
resolver 127.0.0.11 valid=10s ipv6=off;
set $lumo_api_upstream lumo-api:8443;

location = /health {
    proxy_pass https://$lumo_api_upstream$request_uri;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
}

location ^~ /v2/ {
    client_max_body_size 1m;
    proxy_pass https://$lumo_api_upstream$request_uri;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header Connection "";
}

location ^~ /v1/ { return 410; }
```

## Despliegue

Preparación sin iniciar contenedores:

```bash
./scripts/deploy.sh --prepare-only
```

Despliegue y healthcheck:

```bash
export LUMO_TLS_HOSTNAME=api.example.com
export LUMO_PUBLIC_URL=https://api.example.com
# Opcional: comprueba que otro sitio servido por el mismo proxy no cambie.
export LUMO_GUARD_URL=https://example.com
./scripts/deploy.sh
curl --fail --silent --show-error https://api.example.com/health
```

`LUMO_PUBLIC_URL` es obligatorio: el despliegue no se acepta hasta comprobar el health v2, el
`410` de `/v1/state` y el `405` de `GET /v2/groups` a través del proxy real. El script también
rechaza puertos que no estén limitados a loopback y comprueba que el overlay no publique ninguno.

Si ya existe un contenedor o volumen, el script guarda `.env`, el entorno efectivo anterior y una
copia coherente de `/data` en `backups/<fecha>`, y etiqueta la imagen anterior como
`lumo-api:rollback-<fecha>`. En una actualización v2 normal, si falla un gate posterior vuelve a la
imagen y entorno v2 anteriores usando el volumen vivo, y sólo declara éxito si el Nginx real vuelve
a cumplir health v2, `v1=410` y `GET groups=405`. La copia previa nunca se escribe automáticamente
sobre el volumen: el candidato pudo confirmar escrituras después del cutover y rebobinarlo las
perdería. Sólo detiene o recrea `lumo-api`; nunca usa `down`, `--remove-orphans` ni modifica Nginx
u otros servicios del portfolio.

### Primer cutover de v1 a v2

La imagen v1 no es un rollback operativo detrás de un Nginx que devuelve `410` para `/v1/`. Este
primer cambio requiere ventana de mantenimiento y la opción explícita `--first-v2-cutover`. Antes
de actualizar el checkout, captura la identidad y conserva la credencial v1 existente:

```bash
cid="$(docker ps --all --quiet --filter label=com.docker.compose.service=lumo-api)"
test -n "$cid" && test "$(wc -w <<<"$cid")" -eq 1
project="$(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' "$cid")"
data_volume="$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}' "$cid")"
test -n "$project" && test -n "$data_volume"

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -m 0700 -p "backups/pre-v2-$stamp"
install -m 0600 .env "backups/pre-v2-$stamp/env.v1"
umask 077
{
  printf 'COMPOSE_PROJECT_NAME=%s\n' "$project"
  printf 'LUMO_DATA_VOLUME=%s\n' "$data_volume"
  printf 'LUMO_SERVER_MASTER_KEY=%s\n' "$(openssl rand -hex 32)"
  printf 'LUMO_ENABLE_LEGACY_V1=false\n'
} >>.env
chmod 0600 .env
```

Precalienta la compilación mientras v1 todavía sirve tráfico, instala y valida la configuración
Nginx anterior, bloquea `/v1/` y ejecuta el cutover:

```bash
git pull --ff-only
docker compose build --pull lumo-api
docker exec nombre_contenedor_nginx nginx -t
docker exec nombre_contenedor_nginx nginx -s reload
export LUMO_TLS_HOSTNAME=api.example.com
export LUMO_PUBLIC_URL=https://api.example.com
./scripts/deploy.sh --first-v2-cutover
```

Si falla después de recrear, el script detiene `lumo-api`, conserva el volumen vivo y la copia, y
sale con error; no restaura v1 ni anuncia un rollback sano. Revertir a v1 exige una recuperación
manual en mantenimiento que restaure también la configuración Nginx v1. Las filas v1 se conservan
en SQLite, pero no se exponen como grupos v2: los clientes deben realizar un emparejamiento v2 nuevo.

Respuesta esperada:

```json
{"status":"ok","apiVersion":"v2"}
```

## Configuración del cliente

La compilación móvil sólo necesita el origen público:

```dotenv
LUMO_RUNTIME_MODE=remote
LUMO_API_URL=https://api.example.com
```

No declares `LUMO_SERVER_MASTER_KEY`, tokens de dispositivo ni PIN en el entorno del cliente o en
variables `VITE_*`. La credencial revocable se obtiene al crear o consumir una invitación y se
protege con Android Keystore.

## Contrato v2

- `POST /v2/groups`: crea grupo y controlador; requiere un `requestId` estable y está limitado por
  IP, cuota global y máximo de grupos.
- `GET|PUT /v2/groups/{groupId}/state/compact`: estado canónico cifrado, accesible sólo por el
  controlador, con `ETag` y CAS por revisión.
- `GET /v2/groups/{groupId}/member`: vista cifrada de mínimo privilegio para el controlado. No
  contiene PIN, clave del controlador, lugares, historial ni comandos.
- `POST /v2/groups/{groupId}/member/operations`: operaciones tipadas e idempotentes del controlado
  (ubicación, conectividad, seguimiento y ayuda); nunca acepta un estado arbitrario.
- `POST /v2/groups/{groupId}/verify-pin`: valida una acción protegida para un dispositivo activo.
- `POST /v2/groups/{groupId}/invitations`: sólo controlador y PIN correcto.
- `POST /v2/invitations/{invitationId}/consume`: token de 256 bits, PIN, `requestId`, caducidad y
  consumo único.
- `GET /v2/groups/{groupId}/devices`: lista dispositivos activos del grupo.
- `DELETE /v2/groups/{groupId}/devices/{deviceId}`: sólo controlador; revoca un controlado tras
  comprobar el PIN.
- `POST /v2/groups/{groupId}/leave`: revoca el propio dispositivo controlado con PIN.
- `DELETE /v2/groups/{groupId}`: elimina el grupo desde el controlador con PIN.

Las rutas autenticadas usan token Bearer por dispositivo, identificador, fecha y nonce. SQLite
guarda un hash con clave del token, no el token recuperable. El controlador recibe la clave del
estado canónico y cada controlado una clave de miembro distinta. Las respuestas de alta y consumo
se guardan cifradas durante 24 horas para que repetir el mismo `requestId` tras perder una respuesta
no cree dispositivos huérfanos.

Las claves se conservan envueltas con `LUMO_SERVER_MASTER_KEY`. El contenido está cifrado en disco
y en los sobres de aplicación, pero el servidor puede desenvolverlo para aplicar operaciones de
ubicación y geocercas: este diseño no pretende ser cifrado de extremo a extremo frente al operador
del VPS.

## Datos y límites

- Eventos visibles: 24 horas y máximo definido por el dominio.
- Ubicación: sólo la muestra más reciente en el estado; las muestras antiguas no hacen retroceder
  la posición.
- Invitaciones, nonces, respuestas idempotentes y ventanas de rate limit: limpieza automática.
- Estado cifrado: tamaño máximo 512 KiB por grupo.
- Logs Docker: tres archivos de 10 MiB en ambos modos de despliegue.

Los grupos activos no se eliminan a las 24 horas. Su tamaño está acotado y se eliminan mediante
la acción protegida del controlador.

## Operación y copia de seguridad

```bash
docker compose ps
docker compose logs --tail=100 lumo-api
git pull --ff-only
export LUMO_TLS_HOSTNAME=api.example.com
export LUMO_PUBLIC_URL=https://api.example.com
./scripts/deploy.sh
```

Copia coherente de SQLite:

```bash
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "backups/$timestamp"
docker compose stop lumo-api
docker compose cp lumo-api:/data/. "backups/$timestamp/"
docker compose start lumo-api
git rev-parse HEAD >"backups/$timestamp/git-revision.txt"
```

Guarda por separado una copia cifrada de `.env`. Sin el mismo `LUMO_SERVER_MASTER_KEY` no pueden
recuperarse las claves de estado existentes. La rotación destructiva exige copia, estado nuevo y
nuevo emparejamiento; no cambies la clave durante una actualización normal.

Cada directorio automático contiene `data/`, `env.backup`, `previous-container.env`, el identificador
de la imagen previa y `rollback-compose.yml`. No borres estos artefactos hasta validar la aplicación
desde un cliente real. `data/` es recuperación manual, no rollback automático. Si el rollback v2 o
el primer cutover fallan, conserva el volumen y los artefactos sin ejecutar `down -v`; revisa los
logs y restaura primero en un volumen aislado antes de sustituir datos vivos.

## Diagnóstico

- `401 authentication_failed`: token ausente, inválido o revocado.
- `403 unauthorized`: el rol no puede ejecutar esa operación.
- `403 tracking_disabled`: el dispositivo sigue emparejado, pero el seguimiento está apagado.
- `409 replay_detected`: nonce repetido.
- `409 idempotency_conflict`: un `requestId` se reutilizó con datos diferentes.
- `409 revision_conflict`: recargar el estado y repetir la operación.
- `429 rate_limited`: esperar a que venza la ventana o revisar los límites.
- `/health` responde pero la app no conecta: comprobar `LUMO_API_URL`, certificado, proxy y reloj.
