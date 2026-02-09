use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, exit},
};

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
    let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    for path in ["src", "assets", "index.html"].iter() {
        let p = Path::new(&out_dir).join(format!("src/webpage/{}", path));
        println!("cargo:rerun-if-changed={}", p.display());
    }
    let params_src =
        Path::new(&out_dir).join("src/lib/drivers/rest/autopilot/parameters/ardupilot_parameters");
    println!("cargo:rerun-if-changed={}", params_src.display());

    vergen_gix::Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&GixBuilder::all_git()?)?
        .add_instructions(
            CargoBuilder::all_cargo()?.set_dep_kind_filter(Some(DependencyKind::Normal)),
        )?
        .emit()?;

    gzip_ardupilot_parameters(&out_dir)?;

    let dist_dir = Path::new(&out_dir).join("src/webpage/dist");
    if std::env::var("SKIP_FRONTEND").is_ok() {
        fs::create_dir_all(&dist_dir)?;
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

    info!("Building frontend...");
    let mut trunk_command = Command::new("trunk");
    trunk_command
        .current_dir(Path::new(&out_dir).join("src/webpage"))
        .args(["build"]);

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

    if dist_dir.is_dir() {
        let n = gzip_dist_assets(&dist_dir)?;
        info!("Gzipped {n} frontend assets in {dist_dir:?}");
    } else {
        fs::create_dir_all(&dist_dir)?;
        info!("Trunk did not create dist; created empty dir so include_dir! can proceed");
    }

    Ok(())
}

/// Gzip every file in dist so we only embed .gz (smaller binary + transfer). Returns count of files gzipped.
fn gzip_dist_assets(dist_dir: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let files = collect_files(dist_dir)?;
    let n = files.len();
    for path in &files {
        let status = Command::new("gzip")
            .arg("-f") // overwrite if .gz exists
            .arg(path)
            .status()?;
        if !status.success() {
            eprintln!("cargo:warning=gzip failed for {path:?}");
        }
    }
    Ok(n)
}

/// Collect all regular files under `dir` (recursive), skipping `.gz` files.
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_files(&path)?);
        } else if path.extension().map(|e| e == "gz").unwrap_or(false) {
            // already compressed
            continue;
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

/// Produce gzipped parameter JSONs in ardupilot_parameters_gz
fn gzip_ardupilot_parameters(out_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src =
        Path::new(out_dir).join("src/lib/drivers/rest/autopilot/parameters/ardupilot_parameters");
    let dst = Path::new(out_dir).join("ardupilot_parameters_gz");
    if !src.is_dir() {
        fs::create_dir_all(&dst)?;
        return Ok(());
    }
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(&src)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().unwrap();
        let json_path = path.join("apm.pdef.json");
        if !json_path.is_file() {
            continue;
        }
        let out_sub = dst.join(name);
        fs::create_dir_all(&out_sub)?;
        let gz_path = out_sub.join("apm.pdef.json.gz");
        let output = Command::new("gzip").arg("-c").arg(&json_path).output()?;
        if !output.status.success() {
            eprintln!("cargo:warning=gzip failed for {json_path:?}");
            continue;
        }
        fs::write(&gz_path, output.stdout)?;
    }
    Ok(())
}
