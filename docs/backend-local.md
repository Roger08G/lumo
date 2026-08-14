# Backend de Lumo

Lumo puede funcionar con SQLite local o sincronizar el mismo dominio Rust mediante `lumo-api`.
React no contiene reglas de autorización: llama a comandos Tauri y sólo conserva la última vista
confirmada para poder dibujar la interfaz.

## Módulos

- `lumo-core`: grupos, PIN, invitaciones, geocercas, eventos y reglas de seguimiento.
- `lumo-protocol`: firma HMAC de peticiones y contrato de sobres remotos.
- `lumo-runtime`: SQLite local, cliente HTTPS y tres herramientas de diagnóstico.
- `lumo-api`: servicio Axum con almacenamiento SQLite y control optimista de revisión.
- `mobile/src-tauri`: comandos delgados que reutilizan el runtime.

Cada área Rust expone sus módulos mediante `mod.rs`. Los binarios y los comandos Tauri no duplican
lógica de dominio.

## Protección de los datos

- El PIN se guarda como hash Argon2id y nunca forma parte de `AppSnapshot` ni de `localStorage`.
- Cinco intentos incorrectos bloquean temporalmente las acciones protegidas.
- Las invitaciones usan tokens aleatorios, caducan y sólo se aceptan una vez.
- El estado se sella con XChaCha20-Poly1305 antes de salir del cliente.
- Cada petición se firma con HMAC-SHA256, fecha y nonce; el servidor rechaza firmas antiguas y
  repeticiones.
- El transporte remoto exige HTTPS y el servidor guarda únicamente el sobre cifrado.
- El cliente reutiliza la conexión HTTPS, negocia el formato compacto, usa `ETag` para evitar
  descargas sin cambios y reintenta fallos transitorios sin duplicar mutaciones.
- Los eventos expiran automáticamente después de 24 horas.

La contraseña de API se incorpora al binario Tauri al compilar en modo remoto. Esto sirve para un
despliegue familiar privado, pero no sustituye identidad individual, revocación y claves en Android
Keystore si la APK se distribuye públicamente.

## Configuración del cliente

1. Copia `.env.example` como `.env`.
2. Define `LUMO_RUNTIME_MODE=remote`.
3. Indica la URL HTTPS completa en `LUMO_API_URL`.
4. Genera un secreto aleatorio largo y usa el mismo valor en `LUMO_API_PASSWORD` del cliente y del
   contenedor.
5. Vuelve a compilar la aplicación. El `build.rs` de Tauri incorpora el secreto a la librería
   nativa; Vite recibe solamente el origen público HTTPS para validar los QR.

El modo `local` no abre conexiones y sigue siendo el predeterminado.

## Servidor Docker

El contenedor necesita un certificado y su clave en:

```text
certs/fullchain.pem
certs/privkey.pem
```

Después:

```powershell
docker compose up -d --build
docker compose ps
```

El servicio escucha en `8443`, guarda la base en el volumen `lumo-data` y se ejecuta con un usuario
sin privilegios. La clave privada montada debe ser legible por el UID `10001` dentro del contenedor.

## Verificación local

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-local-backend.ps1
```

```bash
./scripts/verify-local-backend.sh
```

El script ejecuta formato, tipos, Clippy sin advertencias, todas las pruebas, compila el workspace,
genera la API y los tres binarios en modo release, y lanza sus pruebas autónomas. Los ejecutables quedan en
`C:\.android\lumo-target\release`.

## Límites actuales

- La API v1 guarda un solo estado cifrado y usa una credencial compartida. Es adecuada para una
  beta familiar privada, no para una distribución pública ni para alojar varios grupos aislados.
- Extraer una APK permite recuperar la credencial incorporada. Producción pública requiere
  identidad y clave no exportable por dispositivo, autorización por rol en el servidor, revocación
  y almacenamiento separado por grupo.
- El servicio Android mantiene una cola cifrada y seguimiento en primer plano, pero Doze, un
  `force-stop` o restricciones agresivas del fabricante pueden retrasar el sondeo. Las pruebas en
  dispositivos físicos siguen siendo obligatorias.
- Android siempre permite revocar permisos desde el sistema; ninguna aplicación legítima puede
  impedirlo de forma absoluta.
