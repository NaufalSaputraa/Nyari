use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=ui/dist/index.html");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    // Create a minimal placeholder ui/dist/index.html only when missing so
    // `include_dir!("ui/dist")` compiles without overwriting real builds.
    let ui_dist_dir = manifest_dir.join("ui").join("dist");
    let ui_index = ui_dist_dir.join("index.html");
    if !ui_index.exists() {
        fs::create_dir_all(&ui_dist_dir).expect("failed to create ui/dist directory");
        fs::write(
            &ui_index,
            "<!doctype html><html><body>Nyari placeholder</body></html>",
        )
        .expect("failed to write placeholder ui/dist/index.html");
        println!("cargo:warning=ui/dist/index.html missing; created placeholder for build/test");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_os == "windows" {
        let app_manifest = manifest_dir.join("app.manifest");
        if !app_manifest.exists() {
            panic!("app.manifest missing at {}", app_manifest.display());
        }

        if target_env == "msvc" {
            // Embed PerMonitorV2 application manifest into linked Windows executables (bin and test targets).
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
                app_manifest.display()
            );
        } else {
            println!(
                "cargo:warning=Non-MSVC Windows target ({target_env}) may require WebView2Loader.dll alongside the executable"
            );
        }
    }
}
