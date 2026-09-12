# Correcciones locales de dependencias

## GLib 0.18.5: RUSTSEC-2024-0429

La cadena GTK de Tauri utiliza GLib 0.18. Se conserva esa API y se incorpora la
corrección de `VariantStrIter`: la función C escribe en un puntero de salida que
Rust debe declarar y prestar como mutable. El defecto puede provocar un fallo de
memoria al optimizar. Referencias: [aviso RustSec](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
y [corrección upstream](https://github.com/gtk-rs/gtk-rs-core/pull/1343).

La copia local mantiene la versión **0.18.5**. No afirma ser una publicación 0.20
ni modifica la base de avisos. La corrección se comprueba sobre el código que
compila Cargo y mediante pruebas en modo `release`.

### Procedencia e integridad

- Paquete original: [glib-0.18.5.crate](https://static.crates.io/crates/glib/glib-0.18.5.crate).
- SHA-256: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.
- Commit de origen: `42b9caf98e03ded086362d9653ca58fe94dc8658`.
- Commit de la corrección: `05dff0ee696f9bcd8617cd48c4b812d046d440cb`.
- Los 121 archivos del paquete se conservan, incluidos `LICENSE`, `COPYRIGHT`
  y `.cargo_vcs_info.json`. Licencia MIT.
- [BACKPORT.json](../vendor/glib/BACKPORT.json) registra los archivos añadidos y
  los hashes de cada archivo original modificado.
- La configuración Git local conserva los bytes originales también al hacer
  checkout en Windows.

El único cambio de producción está en `src/variant_iter.rs`: `let p` pasa a
`let mut p` y el argumento `&p` pasa a `&mut p`. Son las dos líneas de upstream.

También se reparan cinco comprobaciones dentro de `#[cfg(test)]` en
`src/collections/strv.rs`. El test original usaba `get_unchecked(4)` sobre un
slice de longitud cuatro para inspeccionar el terminador de su reserva de
memoria. Eso incumple el contrato de `get_unchecked`. Las comprobaciones leen
ahora el terminador mediante el puntero a la reserva, que sí lo contiene. Este
ajuste afecta exclusivamente a pruebas y permite ejecutar la suite completa
con los optimizadores actuales.

### Reproducción

En Ubuntu se necesitan Rust, Python 3, `pkg-config` y `libglib2.0-dev`. Desde la
raíz del repositorio:

```sh
python3 vendor/glib/verify_backport.py
python3 vendor/glib/test_verify_backport.py
cargo test --manifest-path vendor/glib/Cargo.toml --release --locked \
  --lib --test variant_str_iter_backport --target-dir target/backports
```

El verificador descarga el archivo oficial y comprueba su SHA-256 antes de
comparar **todos** los archivos, aplicando únicamente las transformaciones
documentadas. Para una comprobación sin red se puede pasar
`--archive /ruta/glib-0.18.5.crate`. El `Cargo.lock` local del paquete fija las
dependencias de esta suite independiente; el workspace mantiene su propio lock.

Validación local del 12 de septiembre de 2026: Ubuntu 24.04 x86_64, Rust 1.97.1
y GLib 2.80.0. Pasan las **226 pruebas unitarias upstream y las dos regresiones**
de iteración. En una copia de control se restauró el archivo `variant_iter.rs`
original, verificando su hash, y las mismas regresiones en `release` reprodujeron
`SIGSEGV`. La reparación evita ese fallo conservando la semántica de iteración
hacia delante, hacia atrás, vacía, unitaria y con texto Unicode.

Los avisos de estilo del código upstream con compiladores recientes se conservan
para mantener pequeño el cambio de producción. Esta comprobación no sustituye
la auditoría del resto del grafo de dependencias.

### Mantenimiento

El workspace debe resolver `glib` con `[patch.crates-io]` hacia `vendor/glib` y
excluir esa carpeta de sus miembros para conservar intacto el formato upstream.
Cuando GTK/Tauri admita una versión compatible que incorpore la corrección,
retirar conjuntamente el parche, la copia local y su excepción de workspace,
actualizar los locks y volver a ejecutar las pruebas y la auditoría.

El verificador usa comprobaciones explícitas, que permanecen activas con
`python -O` y `PYTHONOPTIMIZE`. Las regresiones rechazan un archivo de control
alterado en los tres modos, rutas que salgan de la carpeta y enlaces simbólicos.
Las lecturas del archivo original están limitadas a 4 MiB; el hash se comprueba
antes de abrir el contenedor tar.
