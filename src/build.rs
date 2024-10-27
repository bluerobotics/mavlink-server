use std::process::{exit, Command};

use vergen_gix::{BuildBuilder, CargoBuilder, DependencyKind, GixBuilder};

macro_rules! info {
    ($($tokens: tt)*) => {
        println!("cargo:warning={}", format!($($tokens)*))
    }
}

fn is_wasm_target_installed() -> bool {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("Failed to execute rustup");

    let installed_targets = String::from_utf8_lossy(&output.stdout);
    installed_targets.contains("wasm32-unknown-unknown")
}

fn install_wasm_target() {
    info!("Adding wasm32-unknown-unknown target...");
    let output = Command::new("rustup")
        .args(["target", "add", "wasm32-unknown-unknown"])
        .output()
        .expect("Failed to execute rustup");

    if !output.status.success() {
        eprintln!(
            "Failed to install webasm: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(1);
    }
}

fn get_trunk_version() -> Option<String> {
    Command::new("trunk")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|version_string| version_string.split_whitespace().last().map(String::from))
}

fn install_trunk() -> Result<(), Box<dyn std::error::Error>> {
    info!("Installing trunk...");

    let output = Command::new("cargo")
        .arg("install")
        .arg("trunk")
        .arg("--force")
        .output()?;

    if !output.status.success() {
        eprintln!(
            "Failed to install trunk: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(1);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=./src/webpage/");

    vergen_gix::Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&GixBuilder::all_git()?)?
        .add_instructions(
            CargoBuilder::all_cargo()?.set_dep_kind_filter(Some(DependencyKind::Normal)),
        )?
        .emit()?;

    if std::env::var("SKIP_FRONTEND").is_ok() {
        return Ok(());
    }

    if !is_wasm_target_installed() {
        install_wasm_target();
    }

    if get_trunk_version().is_none() {
        info!("trunk not found");
        install_trunk().unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            exit(1);
        });
    }

    let mut trunk_command = Command::new("trunk");
    trunk_command.args(["build", "./src/webpage/index.html"]);

    // Add --release argument if not in debug mode
    if cfg!(not(debug_assertions)) {
        trunk_command.args(["--release", "--locked"]);
    }

    let trunk_output = trunk_command.output().expect("Failed to execute trunk");

    if !trunk_output.status.success() {
        eprintln!(
            "Trunk build failed: {}",
            String::from_utf8_lossy(&trunk_output.stderr)
        );
        exit(1);
    }
    info!("{}", String::from_utf8_lossy(&trunk_output.stdout));

    Ok(())
}
