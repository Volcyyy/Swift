fn main() {
    // `option_env!("BUNGIE_API_KEY")` is expanded at compile time, but cargo
    // does not track environment variables as build inputs on its own. Without
    // this, a binary built with one key is silently reused after the key
    // changes -- which shows up much later as "The given Platform API Key is
    // invalid".
    println!("cargo:rerun-if-env-changed=BUNGIE_API_KEY");

    let key = std::env::var("BUNGIE_API_KEY").unwrap_or_default();

    // A release build is what users install, and they have no way to supply a
    // key themselves, so a missing one has to fail here rather than ship an
    // executable that only reveals the problem at runtime.
    //
    // Debug builds stay buildable without it: `consts::api_key()` reads the
    // environment first, so a key can be supplied when running instead, and
    // `cargo test` needs no key at all.
    if std::env::var("PROFILE").as_deref() == Ok("release") && key.trim().is_empty() {
        panic!(
            "BUNGIE_API_KEY is not set. Release builds compile the key in, so set it before building:\n    PowerShell:  $env:BUNGIE_API_KEY = \"<key>\"\n    bash:        export BUNGIE_API_KEY=<key>\nGet a key at https://www.bungie.net/en/Application"
        );
    }

    tauri_build::build()
}
