/// Ensure `.build/assets/` exists so `include_dir!()` never panics.
///
/// Real assets are created by `just build`; this script only creates
/// empty stubs when they are missing (CI lint / test, local dev).
use std::fs;
use std::path::PathBuf;

fn main() {
    let assets: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", ".build", "assets"]
        .iter()
        .collect();

    if !assets.is_dir() {
        create_stubs(&assets);
    }

    println!("cargo::rerun-if-changed=../.build/assets");
}

fn create_stubs(assets: &PathBuf) {
    fs::create_dir_all(assets).unwrap_or_else(|e| panic!("create {}: {e}", assets.display()));
    for (name, content) in [
        ("cloud-init.yaml", [].as_slice()),
        ("image-digests.json", b"{}".as_slice()),
        ("polis-setup.config.tar", &[0u8; 1024] as &[u8]),
    ] {
        fs::write(assets.join(name), content).unwrap_or_else(|e| panic!("write {name}: {e}"));
    }
}
