use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");

    // Secrets (Wi-Fi credentials, API key) live in the gitignored `.env` at
    // the repo root (KEY=VALUE lines; see .env.example) and are baked into
    // the binary at compile time via cargo:rustc-env + env!().
    let dotenv = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../.env");
    println!("cargo:rerun-if-changed={}", dotenv.display());
    let text = fs::read_to_string(&dotenv).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\ncopy .env.example to .env and fill it in",
            dotenv.display()
        )
    });
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            panic!("{}:{}: expected KEY=VALUE", dotenv.display(), lineno + 1);
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        println!("cargo:rustc-env={}={}", key.trim(), value);
    }
}
