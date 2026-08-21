# Seguridad

## Versiones mantenidas

La rama `main` y la última release estable reciben correcciones de seguridad. Las versiones anteriores no tienen soporte garantizado.

## Comunicar una vulnerabilidad

Usa **Security → Advisories → Report a vulnerability** en GitHub. No publiques una incidencia con credenciales, ubicaciones, PIN, códigos QR, teléfonos ni datos de un servidor real.

Incluye una descripción breve, impacto, versión afectada y pasos mínimos para reproducirlo con datos ficticios. No pruebes una vulnerabilidad contra dispositivos, cuentas o servidores que no controles.

## Secretos y compilaciones

- No adjuntes `.env`, APK, AAB, certificados, claves privadas, keystores o bases de datos.
- El cliente sólo necesita el modo de ejecución y el origen HTTPS público de la API.
- `LUMO_SERVER_MASTER_KEY`, tokens de dispositivo y claves TLS pertenecen exclusivamente al servidor.
- Si una credencial aparece en un commit, log o artefacto, considérala expuesta: revócala o rótala antes de eliminar el contenido.

Las pull requests y los pushes se analizan automáticamente para detectar secretos.
