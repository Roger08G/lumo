fn main() {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    println!("cargo:rerun-if-changed={}", env_path.display());
    let _ = dotenvy::from_path(env_path);
    let mobile_release = std::env::var("PROFILE").as_deref() == Ok("release")
        && matches!(
            std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
            Ok("android") | Ok("ios")
        );
    if mobile_release {
        let mode = std::env::var("LUMO_RUNTIME_MODE").unwrap_or_default();
        assert!(
            mode.eq_ignore_ascii_case("remote"),
            "mobile release requires LUMO_RUNTIME_MODE=remote"
        );
        let api_url = std::env::var("LUMO_API_URL").unwrap_or_default();
        assert!(
            api_url.starts_with("https://") && api_url.len() > "https://".len(),
            "mobile release requires an HTTPS LUMO_API_URL"
        );
    }
    for key in ["LUMO_RUNTIME_MODE", "LUMO_API_URL"] {
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
