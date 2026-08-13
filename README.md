# Lumo

Family location, simply.

## Estructura

```text
crates/
  lumo-core/       Dominio, PIN, invitaciones, geocercas y eventos
  lumo-protocol/   Contrato cifrado y autenticación de la API
  lumo-runtime/    Persistencia local/remota y binarios de diagnóstico
  lumo-api/        Relay Axum con almacenamiento opaco
mobile/            Aplicación Tauri + React
scripts/           Verificación reproducible del backend
```

El workspace funciona sin red mediante SQLite cifrado y queda preparado para usar el mismo dominio
contra `lumo-api` por HTTPS. La configuración y los límites actuales están documentados en
[`docs/backend-local.md`](docs/backend-local.md). El procedimiento de servidor está en
[`docs/deploy.md`](docs/deploy.md).

## Desarrollo

Frontend:

```powershell
cd mobile
bun install --frozen-lockfile
bun run fmt
bun run build
```

Backend Rust:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-local-backend.ps1
```

```bash
./scripts/verify-local-backend.sh
```

El script ejecuta formato, tipos, Clippy, pruebas, builds y los `self-test` de `lumo-controller`,
`lumo-controlled` y `lumo-debug`.

## Android

El wrapper de Tauri configura el toolchain Android del entorno local:

```powershell
cd mobile
bun run tauri android init
bun run tauri android dev
```
