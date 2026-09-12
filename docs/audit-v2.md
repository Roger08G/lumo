# Auditoría técnica de Lumo 2.0.0

Esta revisión cubre el código de la API, el dominio Rust, los repositorios,
el puente Tauri, los servicios Android y el frontend. Se añadieron regresiones
para los fallos identificados y se revisaron los límites de seguridad y
persistencia. La estética de la aplicación se conserva.

Se encontraron rutas de código que explican una pérdida de configuración y el
bloqueo posterior al intentar vincular de nuevo el controlado. La revisión no
incluyó una extracción forense del teléfono afectado: permite demostrar los
fallos del código y sus correcciones, pero no atribuir cada incidencia de un
dispositivo real a una causa única.

## Incidencia principal y recuperación

Había dos problemas que podían encadenarse:

1. Un fallo al leer Android Keystore podía tratarse como ausencia de credencial
   y provocar su eliminación. Un error de transporte, cifrado o inicialización
   tampoco debía interpretarse automáticamente como una revocación.
2. La API conservaba el registro del controlado y sólo permitía uno activo por
   grupo. El teléfono sin credenciales no podía volver a incorporarse, y una
   invitación normal tampoco resolvía ese registro anterior.

Ahora los fallos de lectura preservan la clave y el contenido cifrado, y el
arranque puede reintentarse. La señal remota `credential_rejected` identifica
el rechazo explícito de una credencial. Los errores de red, respuestas ajenas
al contrato y fallos criptográficos siguen un tratamiento recuperable, sin
confundirse con esa señal.

Si los datos del teléfono se perdieron de forma permanente, el controlador
puede emitir una invitación de recuperación vinculada al identificador del
controlado anterior mediante `replaceDeviceId`. Crear el QR no lo desconecta.
Al consumirlo con token y PIN válidos, la API revoca la credencial anterior y
vincula el teléfono recuperado dentro de una transacción SQLite. Se conserva
el grupo y su estado; no es necesario crear otra organización.

Las regresiones comprueban PIN incorrecto, rol no autorizado, otro grupo,
dispositivo equivocado, QR obsoleto, solicitudes concurrentes y fallo de
almacenamiento durante el reemplazo. También comprueban que la credencial
anterior pierde acceso y que un reintento idempotente no crea otro dispositivo.

Para resolver la incidencia desde la aplicación:

1. Actualizar primero la API y después los clientes que usarán la recuperación.
2. En el teléfono controlador, abrir la invitación del grupo y seleccionar
   reconectar o sustituir el controlado existente, usando el PIN del grupo.
3. En el controlado, escanear ese QR e introducir el PIN.
4. Completar los permisos y la configuración de seguimiento del teléfono
   recuperado; comprobar que el controlador recibe una ubicación nueva.

La recuperación necesita un controlador autorizado y el PIN. No recupera un
grupo cuya clave maestra del servidor se haya perdido.

## Correcciones por capa

| Área | Fallo identificado | Comportamiento corregido |
| --- | --- | --- |
| API | Autenticación y escritura separadas permitían una carrera con la revocación | Las operaciones comprueban que el dispositivo sigue autorizado al acceder o modificar el estado protegido; las escrituras se validan dentro de su transacción |
| Invitaciones | Una invitación pendiente podía sobrevivir a la revocación de su emisor | El consumo exige un controlador emisor activo; el reemplazo exige además el controlado concreto que se autorizó sustituir |
| Reintentos de incorporación | Una respuesta idempotente podía devolver credenciales ya revocadas | La API comprueba que la credencial conservada para el reintento sigue activa |
| Runtime HTTP | Un error HTTP genérico podía confundirse con una desvinculación | La clasificación utiliza el código de error del protocolo y conserva las operaciones con resultado ambiguo para reintentos idempotentes |
| Persistencia local | El reemplazo de archivos podía perder el archivo válido si fallaba la instalación del nuevo | Las escrituras preparan el reemplazo antes de instalarlo, sin borrar previamente el archivo válido |
| Ciclo de vida Android | Respuestas o tareas de una credencial anterior podían afectar a una configuración nueva | Las acciones de segundo plano contrastan la identidad y la credencial vigentes antes de modificar servicios, cola o notificaciones |
| Android 7–10 | Los callbacks del proveedor GPS podían depender de métodos que sólo tienen implementación desde Android 11 | El servicio utiliza `LocationListenerCompat`; una regresión comprueba la implementación de los callbacks antiguos |
| Pausa y configuración | Un tick pendiente podía reactivar el seguimiento; el estado remoto podía ocultar la guía en un teléfono reemplazado | Se respeta la pausa explícita y el onboarding exige la configuración local de la instalación actual |
| Cola Android | Una cola temporalmente ilegible podía impedir entregar la muestra actual | Se conserva su contenido cifrado para otro intento y se separa ese fallo de la entrega actual; el vaciado tiene un límite por ejecución |
| Localización | Una muestra antigua descargada de la cola podía completar una petición nueva | La muestra debe haberse capturado desde la creación de la petición; si no existe una muestra válida, se informa `location_unavailable` |
| Seguimiento | Activar o desactivar el seguimiento podía desordenar cambios remotos y del servicio nativo | Las transiciones se serializan; una activación nativa fallida intenta compensar la activación remota |
| Sesión del controlador | El cliente bloqueaba salir del grupo aunque la API admitía esa operación | Se admite la salida autorizada; el servidor conserva su regla para la salida del último controlador |
| Frontend | Una lectura lenta podía sobrescribir una mutación o una lectura posterior | `SnapshotGuard` ordena mutaciones y descarta lecturas obsoletas |
| Estado de presentación | Datos antiguos o duplicados en almacenamiento WebView podían reaparecer al arrancar | El cliente nativo hidrata los datos del backend y limpia copias anteriores de grupo, ubicación y eventos en almacenamiento web |
| Coordenadas | Cadenas ambiguas o fuera de rango podían llegar al guardado de lugares | El análisis valida formato, números finitos y rangos geográficos antes de enviar la operación |

Las pruebas específicas están en los módulos del dominio, los flujos remotos,
las [regresiones de recuperación de la API](../crates/lumo-api/tests/api_flow/recovery.rs),
los tests del plugin Android y los servicios y estado del frontend.

## Arquitectura y rendimiento

La API v2 mantiene la autoridad del grupo. `lumo-core` concentra las reglas de
negocio; `lumo-protocol` define los contratos; `lumo-runtime` gestiona transporte,
credenciales, caché y persistencia de operaciones. Tauri coordina el ciclo de
vida, y el plugin Android mantiene las responsabilidades del sistema operativo.
El frontend adapta y presenta el estado mediante módulos separados. El detalle
y el flujo de recuperación están en [architecture.md](architecture.md).

La caché permite consultar información anterior durante una interrupción, pero
no sustituye una autorización remota ni confirma una escritura. Las mutaciones
dependen del estado y la revisión aceptados por el servidor.

Los cambios de rendimiento evitan trabajo innecesario o no acotado:

- El sondeo espera a la petición anterior y se suspende cuando la aplicación
  deja de estar visible; los servicios Android gestionan el segundo plano.
- Las pantallas se cargan según se necesitan. En el build local, el JavaScript
  inicial pasa de 469,28 a 377,53 kB (gzip: 145,67 a 121,82 kB); se conserva el CSS.
- La admisión pública limita el tamaño del cuerpo y la cola de solicitudes
  pendientes. El hash de PIN se serializa y conserva su permiso mientras sigue
  ejecutándose, aunque el cliente HTTP se desconecte.
- Un token de invitación incorrecto se rechaza antes del hash de PIN y no
  consume los intentos de PIN del titular del QR.
- Reabrir una base de datos ya migrada no repite el DDL de migración.
- El vaciado de la cola Android procesa un lote acotado y conserva entradas
  pendientes ante errores temporales.

Estas mejoras se apoyan en el flujo de código y sus regresiones. No se presenta
un porcentaje de ahorro de batería, latencia o memoria: faltan mediciones
comparables en el hardware y servidor de producción.

## Seguridad y dependencias

Se mantienen separación de roles, aislamiento entre grupos, claves diferenciadas
para el controlado, protección contra repetición, cuotas, expiración de
invitaciones y protección de PIN por dispositivo. Las comprobaciones añadidas
refuerzan revocación, recuperación y clasificación de errores. La revisión no
demuestra que todas las posibles vulnerabilidades estén ausentes.

### Compatibilidad de PIN: Argon2 0.5 a 0.6

Actualizar la biblioteca no debe invalidar los PIN ni los hashes existentes.
La aceptación de esta transición requiere comprobar hashes producidos con 0.5
desde 0.6, tanto en el dominio como en la API. Deben conservarse Argon2id,
su versión y parámetros, el formato PHC, y la derivación que vincula el PIN
del servidor al grupo y a la clave maestra.

El gate incluye verificar el PIN correcto, rechazar otro PIN y mantener el
rechazo cuando cambian el grupo o la clave maestra. No debe regenerarse la
clave maestra ni solicitar al usuario que cambie su PIN para adaptar una
dependencia. El manifiesto, el lock y las pruebas de compatibilidad son la
evidencia de cierre; un cambio de versión por sí solo no la sustituye.

### GLib y deuda upstream

La cadena GTK de Tauri necesita GLib 0.18. Se incorpora localmente la corrección
de `RUSTSEC-2024-0429` manteniendo la versión 0.18.5 y su licencia. El verificador
contrasta todos los archivos con el paquete oficial de checksum conocido. Las
regresiones optimizadas reproducen el fallo de memoria con el código original
y pasan con la corrección. Procedencia, cambios y reproducción están en
[dependency-backports.md](dependency-backports.md).

El verificador también dispone de controles contra un archivo alterado bajo
Python normal, `-O` y `PYTHONOPTIMIZE`, además de rutas externas y enlaces
simbólicos. La verificación de la copia local complementa el análisis del lock;
no se falsea la versión para aparentar una actualización upstream.

En el grafo Rust revisado persisten **seis avisos RustSec de falta de
mantenimiento**, introducidos transitivamente por la cadena Tauri/GTK:

- `proc-macro-error`.
- `unic-char-property`.
- `unic-char-range`.
- `unic-common`.
- `unic-ucd-ident`.
- `unic-ucd-version`.

Son deuda upstream explícita. Esta auditoría no demostró una vulnerabilidad
explotable de Lumo derivada de esos seis avisos, lo que tampoco garantiza que
sean inocuos. Deben seguir visibles en la revisión del grafo y retirarse mediante
actualizaciones compatibles cuando las dependencias que los introducen lo
permitan.

## Validación y publicación

Los gates de la versión incluyen:

- Formato, Clippy, pruebas del workspace y compilaciones Rust con lock fijo,
  mediante los scripts de verificación del backend.
- Formato, ESLint, TypeScript, regresiones, auditoría y build del frontend.
- Compilación Android, tests unitarios del plugin y Android Lint.
- Verificación de procedencia de GLib, regresiones del verificador y pruebas
  de GLib en modo `release` sobre Linux.
- Escaneo de secretos, revisión de dependencias y comprobación de que los locks
  no cambian durante CI.
- Build del contenedor, revisión de límites y permisos, arranque y contrato
  público de salud de la API en el entorno de prueba.

Los resultados locales no sustituyen la ejecución de Actions sobre el commit
publicado. El cierre de CI y PR debe contrastarse con ese commit; este documento
no asigna un estado final a ejecuciones que todavía estén pendientes.

El release previsto contiene los archivos fuente generados por Git, sin adjuntar
APK. Las reglas de exportación excluyen los archivos `.env*`; las credenciales,
certificados y datos operativos permanecen fuera del código publicado. Se
conservan los recursos necesarios para compilar y las licencias de terceros.

## Límites operativos

No se ha desplegado esta revisión en el VPS ni se han probado los teléfonos
reales durante esta auditoría. Quedan por verificar permisos efectivos,
restricciones de batería de cada fabricante, reinicio del teléfono, recepción
de alarmas, ubicación en segundo plano y recuperación frente a pérdidas de red
en esos dispositivos. Tampoco se certifica una carga sostenida del VPS.

El despliegue debe instalar **primero la API y después el cliente**. Las
invitaciones ordinarias siguen siendo compatibles porque `replaceDeviceId` es
opcional, pero la recuperación explícita necesita ambos componentes actualizados.

La base de datos migra al esquema 6. Un binario anterior que sólo admita el
esquema 5 rechazará esa base de datos; volver a una imagen anterior no constituye
por sí solo un rollback válido. Antes de actualizar se necesita una copia
consistente del volumen y conservar la misma clave maestra, siguiendo
[deploy.md](deploy.md) y un procedimiento de recuperación compatible con el
esquema. No se ha validado ese rollback sobre datos reales.

Publicar el código, el tag o una rama no actualiza el servidor ni las aplicaciones
instaladas. La verificación operativa posterior al despliegue sigue siendo
necesaria; esta revisión reduce fallos demostrados sin prometer ausencia total
de errores.
