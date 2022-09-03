// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility qemu`
//!
//! This command launches qemu with a gdb server
//!
//! The `--port` option can be used to specify the gdb port
//!
//! The `--wait` option instructs qemu to wait for a gdb client to connect
//!
//! The `--gdb` option can be used to launch qemu and then open a gdb console connected to it
//!
//! This works by parsing the qemu.sh file within the chip folder
//! (`<hubris>/chips/<chipname>/qemu.sh`), then adding additional args to configure gdb
//!

use std::fs;
use std::process::{Command, Stdio};

use cmd_gdb::gdb;
use humility_cli::{ExecutionContext, Subcommand};
use humility_cmd::{Archive, Command as HumilityCommand, CommandKind};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

#[derive(Parser, Debug)]
#[clap(
    name = "qemu", about = env!("CARGO_PKG_DESCRIPTION"),
)]
struct QemuArgs {
    /// what port to connect qemu gbd server on, passes -gdb tcp::<port> to qemu
    #[clap(long, short, default_value_t = 3333)]
    port: u16,

    /// wait for gdb connection to boot, passes -S to qemu
    #[clap(long, short)]
    wait: bool,

    /// immediatly start a connected gdb shell
    #[clap(long, short)]
    gdb: bool,
}

fn qemu(context: &mut ExecutionContext) -> Result<()> {
    let hubris = context.archive.as_ref().unwrap();

    let Subcommand::Other(subargs) = context.cli.cmd.as_ref().unwrap();
    let subargs = QemuArgs::try_parse_from(subargs)?;

    //parse port from args, this is the port gdb-server will listen on
    let serv_config = format!("tcp::{}", subargs.port);

    let work_dir = tempfile::tempdir()?;

    // extract bin to run in qemu
    hubris
        .extract_file_to("img/final.bin", &work_dir.path().join("final.bin"))?;

    // extract elf to pass to qemu
    hubris
        .extract_file_to("img/final.elf", &work_dir.path().join("final.elf"))?;

    // extract the ihex as well, this lets the runner choose either format and "just work"
    hubris.extract_file_to(
        "img/final.ihex",
        &work_dir.path().join("final.ihex"),
    )?;

    // extract qemu runner from hubris archive
    hubris
        .extract_file_to("debug/qemu.sh", &work_dir.path().join("qemu.sh"))
        .context("No 'qemu.sh' archive found in the hubris archive")?;
    let qemu_cmd = fs::read_to_string(&work_dir.path().join("qemu.sh"))
        .expect("Could not find 'qemu.sh' command");

    // strip off any lines with comments
    let qemu_cmd_w_comments = qemu_cmd.split('\n');
    let mut qemu_w_args = Vec::new();
    for s in qemu_cmd_w_comments {
        if !s.starts_with('#') {
            // remove \ at end of lines
            let s = if let Some(stripped) = s.strip_suffix('\\') {
                stripped
            } else {
                s
            };
            qemu_w_args.extend(s.trim().split(' ').filter(|s| !s.is_empty()));
        }
    }

    let mut cmd = Command::new(qemu_w_args[0]);
    cmd.current_dir(work_dir.path());

    //skip first word
    for arg in &qemu_w_args[1..] {
        cmd.arg(arg);
    }

    // open gdb port
    cmd.arg("-gdb");
    cmd.arg(serv_config);

    if subargs.wait || subargs.gdb {
        cmd.arg("-S");
    }

    humility::msg!("launching qemu: {:?}", cmd);

    // If running with immediate gdb attachment,  need to run qemu in the "background"
    struct Runner(std::process::Child);
    impl Drop for Runner {
        fn drop(&mut self) {
            self.0.kill().expect("Could not stop 'qemu'");
        }
    }

    // Ignore ctrl c so qemu and or gdb can handle it
    if subargs.gdb {
        // start qemu in the background
        cmd.stdin(Stdio::piped());
        let _qemu = Runner(cmd.spawn().context("Could not start 'qemu'")?);
        // now start gdb
        gdb(context)?;
    } else {
        //turn off ctrl c, qemu can handle it
        ctrlc::set_handler(|| {}).expect("Error setting Ctrl-C handler");
        // Run qemu only
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("could not start qemu");
        }
    };
    Ok(())
}

pub fn init() -> HumilityCommand {
    HumilityCommand {
        app: QemuArgs::command(),
        name: "qemu",
        run: qemu,
        kind: CommandKind::Unattached { archive: Archive::Required },
    }
}
