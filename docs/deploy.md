# Despliegue de `lumo-api`

## Requisitos

- Servidor Linux con Bash, OpenSSL, Docker Engine y Docker Compose v2.
- Un dominio que resuelva a la IP del servidor.
- Reloj del servidor sincronizado mediante NTP. La autenticación rechaza peticiones con más de
  cinco minutos de desfase.
- Puerto TCP `8443` accesible, o el puerto `443` si se configura `LUMO_API_PORT=443`.
- Certificado TLS válido para el dominio usado por la aplicación.

La versión actual mantiene un único estado compartido. Despliega una instancia, volumen, dominio y
secreto independientes por grupo o entorno.

## Archivos del servidor

Desde la raíz del repositorio:

```text
.env
certs/fullchain.pem
certs/privkey.pem
```

`.env` y `certs/` están excluidos de Git y del contexto de construcción de Docker.

Genera un secreto aleatorio y crea el archivo `.env`:

```bash
umask 077
printf 'LUMO_API_PASSWORD=%s\nLUMO_API_PORT=8443\n' "$(openssl rand -hex 32)" > .env
```

No uses espacios, un secreto reutilizado ni una variable `VITE_*`. El secreto debe tener al menos
32 bytes y debe coincidir exactamente con el usado al compilar los clientes.

Copia el certificado y la clave como archivos reales. No uses enlaces simbólicos hacia rutas que no
estén montadas en el contenedor:

```bash
mkdir -p certs
deploy_group="$(id -g)"
sudo install -o 10001 -g "$deploy_group" -m 0444 /ruta/fullchain.pem certs/fullchain.pem
sudo install -o 10001 -g "$deploy_group" -m 0440 /ruta/privkey.pem certs/privkey.pem
```

El proceso se ejecuta con UID `10001`, por lo que necesita permiso de lectura sobre ambos archivos.
El grupo del usuario de despliegue también necesita leer la clave para que el script pueda validarla
sin ejecutarse como `root`; ningún otro usuario recibe acceso a ella.

### Proxy inverso en Docker

Si Nginx se ejecuta en Docker, usa la red externa del proxy y no publiques el puerto de la API en
el host:

```dotenv
COMPOSE_FILE=docker-compose.yml:docker-compose.proxy.yml
LUMO_PROXY_NETWORK=nombre_de_la_red_nginx
```

El overlay `docker-compose.proxy.yml` elimina los puertos publicados, limita memoria, CPU, procesos
y logs, monta el sistema de archivos como sólo lectura y conserva escritura únicamente en `/data`.
Nginx puede resolver el upstream como `https://lumo-api:8443` dentro de la red compartida.

## Primer despliegue

Prepara `.env` y el directorio de certificados sin arrancar servicios:

```bash
./scripts/deploy.sh --prepare-only
```

Instala el certificado como se indica arriba y ejecuta el despliegue:

```bash
./scripts/deploy.sh
```

El script valida el secreto, Docker Compose, la caducidad del certificado, la correspondencia entre
certificado y clave, los permisos de lectura, construye la imagen y espera hasta 60 segundos a que
el contenedor esté saludable.

Comprueba el endpoint TLS desde otra máquina:

```bash
curl --fail --silent --show-error https://api.example.com:8443/health
```

Respuesta esperada:

```json
{ "status": "ok", "apiVersion": "v1" }
```

Si el host publica `443`, usa `LUMO_API_PORT=443` y omite `:8443` en la URL. El contenedor siempre
escucha en `8443`; Compose realiza el mapeo del puerto externo.

## Configuración del cliente

En el `.env` usado para compilar la aplicación:

```dotenv
LUMO_RUNTIME_MODE=remote
LUMO_API_URL=https://api.example.com:8443
LUMO_API_PASSWORD=el-mismo-secreto-del-servidor
```

El nombre de host debe coincidir con el certificado. Después de cambiar estas variables hay que
recompilar e instalar de nuevo la aplicación; no se leen dinámicamente desde JavaScript.

## Operación

Estado y logs:

```bash
docker compose ps
docker compose logs --tail=100 lumo-api
docker compose logs --follow lumo-api
```

Actualización:

```bash
git pull --ff-only
docker compose config --quiet
docker compose build --pull
docker compose up -d --remove-orphans
curl --fail --silent --show-error https://api.example.com:8443/health
```

Renovación del certificado:

```bash
deploy_group="$(id -g)"
sudo install -o 10001 -g "$deploy_group" -m 0444 /ruta/fullchain.pem certs/fullchain.pem
sudo install -o 10001 -g "$deploy_group" -m 0440 /ruta/privkey.pem certs/privkey.pem
docker compose restart lumo-api
```

## Copia de seguridad

El estado persistente está en el volumen `lumo-data`, dentro de `/data`. Detén el servicio antes de
copiar SQLite para incluir de forma consistente la base y sus posibles archivos WAL:

```bash
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "backups/$timestamp"
docker compose stop lumo-api
docker compose cp lumo-api:/data/. "backups/$timestamp/"
docker compose start lumo-api
```

Guarda junto a la copia la versión desplegada:

```bash
git rev-parse HEAD > "backups/$timestamp/git-revision.txt"
```

Verifica periódicamente la restauración en una instancia aislada. La copia de SQLite no sustituye
el secreto: sin el mismo `LUMO_API_PASSWORD`, los clientes no pueden descifrar el estado.

El servidor conserva un único estado cifrado con límites de tamaño; los eventos visibles se filtran
a 24 horas y se limitan a 200. SQLite hace checkpoints automáticos y limita el journal a 16 MiB. El
overlay de proxy rota los logs de Docker a tres archivos de 10 MiB.

## Rotación del secreto

El secreto autentica las peticiones y deriva la clave que cifra el estado remoto. Cambiarlo sólo en
el servidor deja los clientes sin acceso; cambiarlo también en los clientes hace ilegible el estado
ya almacenado.

Esta versión no incluye migración de claves. Una rotación requiere una ventana planificada, copia de
seguridad, reinicio del estado remoto, recompilación de todos los clientes y nuevo emparejamiento.
No elimines el volumen como parte de una actualización normal.

## Diagnóstico

- `health: starting` o `unhealthy`: revisa `docker compose logs lumo-api` y que el proceso pueda leer
  los certificados.
- Error TLS: comprueba dominio, cadena completa, caducidad y puerto publicado.
- `authentication_failed`: el cliente y el servidor no comparten el mismo secreto o sus relojes
  están desincronizados.
- `revision_conflict`: otro dispositivo actualizó el estado; el cliente debe refrescar y reintentar.
- El endpoint `/health` funciona pero la aplicación no conecta: confirma que `LUMO_API_URL` usa
  `https://`, el puerto correcto y el mismo nombre incluido en el certificado.
