# Lumo

Family location, simply.

## Estructura

```text
crates/
  lumo-core/       Dominio, PIN, invitaciones, geocercas y eventos
  lumo-protocol/   Contrato v2, credenciales y sobres cifrados
  lumo-runtime/    Persistencia, cliente HTTPS y binarios de diagnóstico
  lumo-api/        API Axum aislada por grupo y dispositivo
mobile/            Aplicación Tauri + React
plugins/           Integración Android y Android Keystore
scripts/           Verificación reproducible del backend
```

El modo local funciona sin red mediante SQLite. En remoto, cada dispositivo recibe una credencial
revocable durante el emparejamiento y el APK no contiene secretos del servidor. La configuración
está documentada en [`docs/backend-local.md`](docs/backend-local.md). El procedimiento de servidor
está en [`docs/deploy.md`](docs/deploy.md).

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
