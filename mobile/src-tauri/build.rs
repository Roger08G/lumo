fn main() {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    println!("cargo:rerun-if-changed={}", env_path.display());
    let _ = dotenvy::from_path(env_path);
    for key in ["LUMO_RUNTIME_MODE", "LUMO_API_URL", "LUMO_API_PASSWORD"] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = std::env::var(key) {
            assert!(
                !value.contains(['\r', '\n']),
                "{key} cannot contain line breaks"
            );
            println!("cargo:rustc-env={key}={value}");
        }
    }
    tauri_build::build()
}
