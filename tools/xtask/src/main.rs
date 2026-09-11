use std::path::Path;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    anyhow::ensure!(std::env::args().len() == 1, "usage: cargo xtask");

    // hubris wants linker scripts to be in the working tree so let me just paste em in from the crates
    std::env::set_current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let metadata = cargo_metadata::MetadataCommand::new().exec()?;
    let hubris = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "xtask")
        .context("Hubris xtask dependency not found")?;
    let build = hubris.manifest_path.parent().unwrap().join("..");
    std::fs::create_dir_all("build")?;
    for script in [
        "kernel-link.x",
        "task-link.x",
        "task-rlink.x",
        "task-tlink.x",
    ] {
        std::fs::copy(build.join(script), Path::new("build").join(script))?;
    }

    xtask::dist::package(
        Path::new("app.toml"),
        xtask::dist::PackageFlags {
            verbose: false,
            edges: false,
            dirty_ok: false,
            skip_path_check: false,
        },
        None,
        xtask::CabooseArgs::default(),
    )?;
    Ok(())
}
