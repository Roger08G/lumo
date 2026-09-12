# Seguridad

## Versiones mantenidas

La rama `main` y la última release estable reciben correcciones de seguridad. Las versiones anteriores no tienen soporte garantizado.

## Comunicar una vulnerabilidad

Usa **Security → Advisories → Report a vulnerability** en GitHub. No publiques una incidencia con credenciales, ubicaciones, PIN, códigos QR, teléfonos ni datos de un servidor real.

Incluye una descripción breve, impacto, versión afectada y pasos mínimos para reproducirlo con datos ficticios. No pruebes una vulnerabilidad contra dispositivos, cuentas o servidores que no controles.

## Secretos y compilaciones

- No adjuntes `.env`, APK, AAB, certificados, claves privadas, keystores o bases de datos.
- Para compilar el cliente sólo se necesita el modo de ejecución y el origen HTTPS público de la API.
- `LUMO_SERVER_MASTER_KEY` y las claves privadas TLS pertenecen exclusivamente al servidor.
- Las credenciales de dispositivo se emiten al vincularlo y se conservan en su almacén seguro; nunca se incorporan al código ni a la configuración de compilación.
- Si una credencial aparece en un commit, log o artefacto, considérala expuesta: revócala o rótala antes de eliminar el contenido.

Las pull requests y los pushes se analizan automáticamente para detectar secretos.
