use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");

    // Credentials live OUTSIDE the repo so no Wi-Fi password or API key can
    // ever be committed by accident. Override the path with EPAPER_SECRETS;
    // default: ~/.config/esp32-epaper/secrets.rs
    // (copy firmware/src/secrets.rs.example and fill it in).
    let src = match env::var("EPAPER_SECRETS") {
        Ok(p) => PathBuf::from(p),
        Err(_) => PathBuf::from(env::var("HOME").expect("HOME is not set"))
            .join(".config/esp32-epaper/secrets.rs"),
    };
    println!("cargo:rerun-if-env-changed=EPAPER_SECRETS");
    println!("cargo:rerun-if-changed={}", src.display());

    let dst = PathBuf::from(env::var("OUT_DIR").unwrap()).join("secrets.rs");
    fs::copy(&src, &dst).unwrap_or_else(|e| {
        panic!(
            "cannot read credentials file {}: {e}\n\
             copy firmware/src/secrets.rs.example to that path and fill it in",
            src.display()
        )
    });
}
