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
//! The `--silent` option will hide qemu's stdout
//!
//! The `--command` option specifies another humility command to run after launching qemu
//!
//! The `--delay` option is how long to wait, in ms,  before running `command`
//!
//! This works by parsing the qemu.sh file within the chip folder
//! (`<hubris>/chips/<chipname>/qemu.sh`), then adding additional args to configure gdb
//!

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time;

use cmd_gdb::gdb;
use humility::cli::Subcommand;
use humility_cmd::{Archive, Command as HumilityCommand};

use anyhow::{Context, Result};
use clap::{Command as ClapCommand, CommandFactory, Parser};

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

    /// Command to run after starting qemu. Will exit qemu when this command is done
    #[clap(long, short, conflicts_with = "gdb")]
    command: Option<String>,

    /// How long to wait, in milli-seconds, to run `command` after starting qemu
    #[clap(
        long,
        short,
        default_value = "300",
        conflicts_with = "gdb",
        requires = "command"
    )]
    delay: u64,

    /// Hide qemu stdout
    #[clap(long, short)]
    silent: bool,
}

fn qemu(context: &mut humility::ExecutionContext) -> Result<()> {
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
    let mut qemu_cmd = "".to_string();
    for s in qemu_cmd_w_comments {
        if let Some('#') = s.chars().next() {
        } else {
            qemu_cmd += s.trim();
        }
    }
    let qemu_w_args: Vec<&str> = qemu_cmd.split(' ').collect();

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

    if subargs.silent {
        cmd.stdout(Stdio::null());
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
    if subargs.gdb || subargs.command.is_some() {
        // start qemu in the background
        cmd.stdin(Stdio::piped());
        let _qemu = Runner(cmd.spawn().context("Could not start 'qemu'")?);
        if subargs.gdb {
            // now start gdb
            gdb(context)?;
        } else if let Some(command) = subargs.command {
            // we unfornunatly have to contruct a new command from scratch, calling back into the
            // base humility command parsers would create a circular dependency
            let my_humility = std::env::current_exe()?;
            let mut cmd = Command::new(my_humility);
            cmd.current_dir(work_dir.path());
            // setup the correct probe to connect to our lanched qemu
            cmd.arg("-p").arg(format!("qemu-{}", subargs.port));

            // setup correct archive for dump (if avaliable)
            if let Some(_dump) = &context.cli.dump {
                // we do not want to pass through the whole dump, just the archive, so extract it
                // from the dump and pass to subcommand
                let mut buffer =
                    fs::File::create(work_dir.path().join("dump_archive.zip"))?;
                buffer.write_all(hubris.archive())?;
                cmd.arg("-a").arg("dump_archive.zip");
            }

            // setup correct archive (if avaliable)
            if let Some(archive_name) = &context.cli.archive_name {
                cmd.arg("-a").arg(archive_name);
            }
            // setup correct environment (if avaliable)
            if let Some(environment) = &context.cli.environment {
                cmd.arg("-e").arg(environment);
            }

            // add our humility subcommand
            // We split to allow the command to specify additional flags
            for arg in command.split(' ') {
                cmd.arg(arg);
            }

            thread::sleep(time::Duration::from_millis(subargs.delay));

            let status = cmd.status()?;
            if !status.success() {
                anyhow::bail!("command failed: `{}`", command);
            }
        }
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

pub fn init() -> (HumilityCommand, ClapCommand<'static>) {
    (
        HumilityCommand::Unattached {
            name: "qemu",
            archive: Archive::Required,
            run: qemu,
        },
        QemuArgs::command(),
    )
}
