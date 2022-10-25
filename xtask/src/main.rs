// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{bail, Result};
use clap::Parser;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::process::Command;

#[derive(Debug, Parser)]
#[clap(max_term_width = 80, about = "extra tasks to help you work on Hubris")]
enum Xtask {
    /// Compiles readme
    Readme,
}

fn make_readme() -> Result<()> {
    use cargo_metadata::MetadataCommand;
    let mut cmds = BTreeMap::new();

    let metadata =
        MetadataCommand::new().manifest_path("./Cargo.toml").exec().unwrap();

    for id in &metadata.workspace_members {
        let package =
            metadata.packages.iter().find(|p| &p.id == id).unwrap().clone();

        if let Some(cmd) = package.name.strip_prefix("humility-cmd-") {
            let description = match package.description {
                Some(description) => description,
                None => {
                    bail!("{} is missing \"description\" in manifest", cmd);
                }
            };

            cmds.insert(cmd.to_string(), (description, package.manifest_path));
        }
    }

    let root = std::path::PathBuf::from(metadata.workspace_root);
    let input = std::fs::read(root.join("README.md.in"))?;
    let mut output = File::create(root.join("README.md"))?;

    writeln!(
        output,
        "{}",
        r##"<!--
  -- DO NOT EDIT THIS FILE DIRECTLY!
  --
  -- This file is made by running "cargo xtask readme", which pulls in
  -- README.md.in and then automatically concatenates documentation for
  -- each command.  The documentation for the commands is generated by
  -- "cargo readme" in each command crate, and the documentation itself
  -- is written in rustdoc in those crates.
  -->
"##
    )?;

    output.write_all(&input)?;

    writeln!(output, "## Commands\n")?;

    for (cmd, (description, _)) in &cmds {
        writeln!(
            output,
            "- [humility {}](#humility-{}): {}",
            cmd, cmd, description
        )?;
    }

    for (cmd, (_, path)) in &cmds {
        let mut gencmd = Command::new("cargo");
        gencmd.arg("readme");
        gencmd.arg("--no-title");
        gencmd.arg("-r");
        gencmd.arg(path.parent().unwrap());

        let contents = gencmd.output()?;

        if !contents.status.success() {
            bail!(
                "\"cargo readme\" command failed for {}: {:?}; have you run \"cargo install cargo-readme\"?",
                cmd,
                contents
            );
        }

        //
        // We are prescriptive about what we expect this output to look like.
        //
        let header = format!("### `humility {}`\n", cmd);
        if contents.stdout.len() == 1 {
            println!("warning: no documentation for {}", cmd);
            //
            // For now, we offer a cheerful message encouraging documentation --
            // but once we have everything documented, this needs to be a
            // hard failure.
            //
            writeln!(
                output,
                "{}\nNo documentation yet for `humility {}`; \
                pull requests welcome!\n",
                header, cmd
            )?;
        } else {
            if !contents.stdout.starts_with(header.as_bytes()) {
                bail!(
                    "documentation for {} is malformed: \
                    must begin with '{}'",
                    cmd,
                    header
                );
            }

            output.write_all(&contents.stdout)?;
            writeln!(output, "\n")?;
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let xtask = Xtask::parse();

    match xtask {
        Xtask::Readme => {
            make_readme()?;
        }
    }

    Ok(())
}
