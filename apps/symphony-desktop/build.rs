//! On macOS the app's property list (`Info.plist`) is linked into the binary's `__TEXT,__info_plist`
//! section, where the system reads a program's identity and App Transport Security settings when
//! it runs outside an app bundle: the web views then load a tailnet name over plain HTTP.

#![forbid(unsafe_code)]

fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
