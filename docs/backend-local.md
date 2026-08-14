# Backend de Lumo

## Módulos

- `lumo-core`: PIN, grupos, geocercas, eventos y seguimiento.
- `lumo-protocol`: modelos HTTP, sobres cifrados y límites del protocolo.
- `lumo-runtime`: SQLite local, cliente HTTPS, credenciales y binarios de diagnóstico.
- `lumo-api`: Axum, aislamiento por grupo, dispositivos, invitaciones y SQLite.
- `mobile/src-tauri`: autorización por rol y puente Android.
- `tauri-plugin-lumo-mobile`: permisos, servicios, cola offline y Android Keystore.

React dibuja snapshots confirmados y no es autoridad. En Android no persiste grupo, ubicaciones ni
eventos en `localStorage`.

## Seguridad

- PIN Argon2id con cinco intentos y bloqueo temporal.
- Token aleatorio y revocable por dispositivo; la API guarda sólo un hash con clave.
- Invitación QR de un uso, 10 minutos y sin nombres, teléfonos ni PIN.
- Estado canónico XChaCha20-Poly1305 exclusivo del controlador y clave de miembro distinta por
  dispositivo controlado.
- El controlado sólo puede enviar operaciones tipadas; no puede leer el PIN ni reemplazar el estado
  completo del grupo.
- Claves de estado envueltas en servidor; clave maestra exclusiva del VPS.
- Verificador de clave maestra en SQLite: una configuración equivocada impide que la API arranque
  en vez de aceptar un healthcheck engañoso.
- Credencial Android cifrada con AES-256-GCM y clave no exportable de Android Keystore.
- HTTPS obligatorio, redirecciones desactivadas, fecha y nonce persistido contra replay.
- `ETag`, formato compacto, pool HTTPS, reintentos acotados y CAS por revisión.
- Backups Android y tráfico cleartext desactivados.

## Modos

`local` usa SQLite y no abre red. `remote` sólo requiere:

```dotenv
LUMO_RUNTIME_MODE=remote
LUMO_API_URL=https://api.example.com
```

Un cliente remoto sin credencial permanece sin emparejar. Crear un grupo instala la credencial de
controlador; consumir una invitación instala la de controlado. Salir, revocar o eliminar el grupo
borra la credencial local después de que el servidor confirme la operación. Un error de red conserva
el seguimiento y el emparejamiento para poder reintentar.

## Verificación

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-local-backend.ps1
```

```bash
./scripts/verify-local-backend.sh
```

El gate ejecuta formato, Clippy sin avisos, todas las pruebas, builds y `self-test` de los tres
binarios. Android requiere además tests Kotlin, lint, manifest mergeado y prueba física de permisos,
reinicio, Doze, cola offline y revocación.

## Límites operativos

- Estado cifrado: 512 KiB por grupo.
- Eventos: 24 horas; ubicación actual monotónica.
- Cola Android: cifrada, compactada, 24 horas y límite fijo.
- Dispositivos e invitaciones por grupo: configurables en el servidor.
- Doze, `force-stop` y restricciones del fabricante pueden retrasar el sondeo sin push.
- Android siempre permite al usuario revocar permisos; la app debe detectarlo y detenerse de forma
  segura.
- La API usa una sola instancia SQLite y está dimensionada para este despliegue personal; no se
  debe ejecutar en paralelo sobre la misma base de datos.
- El servidor puede desenvolver las claves de grupo para aplicar la lógica de dominio. No es un
  sistema de conocimiento cero y la rotación en línea de la clave maestra no está implementada.
