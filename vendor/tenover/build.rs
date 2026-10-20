use std::io::Result;

fn main() -> Result<()> {
    // r10-gspro patch: use a vendored protoc so no system protobuf install is needed.
    if std::env::var_os("PROTOC").is_none() {
        if let Ok(path) = protoc_bin_vendored::protoc_bin_path() {
            // SAFETY: build scripts are single-threaded.
            unsafe { std::env::set_var("PROTOC", path) };
        }
    }

    prost_build::Config::new().btree_map(["."]).compile_protos(
        &[
            "proto/GDISmart.proto",
            "proto/GDIEventSharing.proto",
            "proto/GDILaunchMonitor.proto",
        ],
        &["proto/"],
    )?;

    // Declare custom cfg names so rustc doesn't warn about them.
    println!("cargo::rustc-check-cfg=cfg(ble_backend_bluez)");
    println!("cargo::rustc-check-cfg=cfg(ble_backend_btleplug)");

    // When `ble` feature is active, auto-select backend by target OS.
    if std::env::var("CARGO_FEATURE_BLE").is_ok() {
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os == "linux" {
            println!("cargo::rustc-cfg=ble_backend_bluez");
        } else {
            println!("cargo::rustc-cfg=ble_backend_btleplug");
        }
    }

    Ok(())
}
