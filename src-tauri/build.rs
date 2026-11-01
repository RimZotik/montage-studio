fn main() {
    // Манифест Windows (Common Controls v6) Tauri встраивает только в exe приложения, а тестовым
    // exe он тоже нужен (иначе падают при запуске с STATUS_ENTRYPOINT_NOT_FOUND) — встраиваем сами
    // во все сборки, свой у Tauri отключаем.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri build");
}
