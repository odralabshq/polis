//! Version command

/// Run the version command.
pub fn run(json: bool) {
    let version = env!("CARGO_PKG_VERSION");

    if json {
        println!(r#"{{"version":"{version}"}}"#);
    } else {
        println!("polis {version}");
    }
}
