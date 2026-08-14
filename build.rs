use std::path::PathBuf;

use clap::CommandFactory;

#[path = "src/bin/cli/mod.rs"]
mod cli;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src/bin/cli/mod.rs");

    let output =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or(std::io::ErrorKind::NotFound)?).join("man");
    if output.exists() {
        std::fs::remove_dir_all(&output)?;
    }
    std::fs::create_dir_all(&output)?;
    clap_mangen::generate_to(cli::Cli::command(), output)
}
