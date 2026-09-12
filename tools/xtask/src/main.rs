use std::path::Path;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
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
        let content = std::fs::read(build.join(script))?;
        let destination = Path::new("build").join(script);
        if std::fs::read(&destination).ok().as_deref() != Some(&content) {
            std::fs::write(destination, content)?;
        }
    }

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let app = match args.first().map(String::as_str) {
        Some("lsp") => {
            let mut clients = Vec::new();
            let mut file = None;
            let mut args = args[1..].iter();
            while let Some(arg) = args.next() {
                if arg == "-c" {
                    clients.push(args.next().context("missing LSP client JSON")?.parse()?);
                } else {
                    anyhow::ensure!(
                        !arg.starts_with('-') && file.is_none(),
                        "usage: cargo xtask lsp [-c <client-json>] <file>"
                    );
                    file = Some(arg.into());
                }
            }
            return xtask::lsp::run(&file.context("missing source file")?, &clients);
        }
        Some("rust-analyzer") => {
            anyhow::ensure!(
                args.len() == 2,
                "usage: cargo xtask rust-analyzer <app.toml>:<task>"
            );
            let (manifest, task_name) = args[1]
                .split_once(':')
                .context("expected <app.toml>:<task>")?;
            anyhow::ensure!(!task_name.contains(':'), "expected <app.toml>:<task>");
            return xtask::rust_analyzer::run(
                None,
                Some(xtask::rust_analyzer::HubrisTargetTask {
                    manifest: manifest.into(),
                    task_name: task_name.into(),
                }),
            );
        }
        None => "q65-max",
        Some("build") => {
            anyhow::ensure!(
                args.len() == 2 && !args[1].starts_with('-'),
                "usage: cargo xtask build <app>"
            );
            args[1].as_str()
        }
        _ => anyhow::bail!("usage: cargo xtask [build <app>|lsp|rust-analyzer]"),
    };
    let manifest = Path::new("apps").join(app).join("app.toml");
    anyhow::ensure!(manifest.is_file(), "unknown app: {app}");

    xtask::dist::package(
        &manifest,
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
