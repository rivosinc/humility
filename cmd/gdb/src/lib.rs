// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility gdb`
//!
//! This command launches GDB and attaches to a running device.
//!
//! By default, the user must be running `openocd` or `pyocd` in a separate
//! terminal.
//!
//! The `--run-openocd` option automatically launches `openocd` based on the
//! `openocd.cfg` file included in the build archive.
//!
//! When using `pyocd`, it must be launched with the `--persist` option,
//! because `humility gdb` connects to it multiple times (once to check the
//! app id, then again to run the console).
//!

use std::process::{Command, Stdio};

use std::fs;
use tempfile::TempDir;

use humility::cli::Subcommand;
use humility_cmd::{Archive, Command as HumilityCmd};
use humility_cmd_openocd::get_probe_serial;

use anyhow::{bail, Context, Result};
use clap::{Command as ClapCommand, CommandFactory, Parser};
use regex::Regex;

#[derive(Parser, Debug)]
#[clap(
    name = "gdb", about = env!("CARGO_PKG_DESCRIPTION"),
)]
struct GdbArgs {
    /// when set, calls `load` and `stepi` upon attaching
    #[clap(long, short)]
    load: bool,

    /// when set, runs an OpenOCD process before starting GDB
    #[clap(long, group = "run_openocd")]
    run_openocd: bool,

    /// specifies the `openocd` executable to run
    #[clap(long, requires = "run_openocd")]
    openocd: Option<String>,

    /// specifies the probe serial number to use with OpenOCD
    #[clap(long, requires = "run_openocd")]
    serial: Option<String>,

    /// skip checking the image id
    #[clap(long, short)]
    skip_check: bool,

    /// what gdb port to connect on
    #[clap(long, short, default_value = "3333")]
    port: u16,
}

fn extract_elf_dir(work_dir: &TempDir) -> Result<String> {
    // load script.gdb into string
    let script = fs::read_to_string(&work_dir.path().join("script.gdb"))?;

    // regex to extract the path where the elf files are,
    // "script.gdb" should contain something like:
    // `add-symbol-file target/my_app/dist/default/kernel`
    // or
    // `add-symbol-file target/my_app/dist/kernel`
    // this extracts the path between the app name and kernel into a capture group. ex: "dist/default" or "dist/"
    //
    let re = Regex::new(r"(?:add-symbol-file) (?:(.*)?(?:kernel))")?;

    // match all regex
    // Should only be one match since we explicitly match kernel
    let mut cap = re.captures_iter(&script);
    // only use first capture group
    let cap = cap.next().context("invalid `script.gdb`")?;

    // within the capture group, the 0th element is the entire match, so our captured path is at
    // index 1
    let path = cap.get(1).context("invalid 'script.gdb'")?.as_str();

    Ok(path.to_owned())
}

pub fn gdb(context: &mut humility::ExecutionContext) -> Result<()> {
    let Subcommand::Other(subargs) = context.cli.cmd.as_ref().unwrap();
    let hubris = context.archive.as_ref().unwrap();

    if context.cli.probe.is_some() {
        bail!("Cannot specify --probe with `gdb` subcommand");
    }

    let subargs = GdbArgs::try_parse_from(subargs)?;
    let serial = get_probe_serial(&context.cli, subargs.serial.clone())?;

    let work_dir = tempfile::tempdir()?;

    hubris
        .extract_file_to(
            "debug/openocd.gdb",
            &work_dir.path().join("openocd.gdb"),
        )
        .context("GDB config missing. Is your Hubris build too old?")?;
    hubris
        .extract_file_to(
            "debug/script.gdb",
            &work_dir.path().join("script.gdb"),
        )
        .context("GDB script missing. Is your Hubris build too old?")?;

    // use the script.gdb to extract the proper elf path
    let elf_dir = match extract_elf_dir(&work_dir) {
        Ok(dir) => dir,
        // fallback to "dist/default" if script.gdb is invalid
        // realistly these means source level debugging won't work, but might
        // as well try...
        Err(_) => "dist/default".to_owned(),
    };
    let elf_dir = &work_dir.path().join(elf_dir);

    std::fs::create_dir_all(elf_dir)?;
    hubris.extract_elfs_to(elf_dir)?;

    hubris
        .extract_file_to("img/final.elf", &work_dir.path().join("final.elf"))?;

    let mut gdb_cmd = None;

    const GDB_NAMES: [&str; 5] = [
        "arm-none-eabi-gdb",
        "riscv32-none-elf-gdb",
        "riscv32-unknown-elf-gdb",
        "gdb-multiarch",
        "gdb",
    ];
    for candidate in &GDB_NAMES {
        if Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::piped())
            .status()
            .is_ok()
        {
            gdb_cmd = Some(candidate);
            break;
        }
    }

    // Select the GDB command
    let gdb_cmd = gdb_cmd.ok_or_else(|| {
        anyhow::anyhow!("GDB not found.  Tried: {:?}", GDB_NAMES)
    })?;

    // If OpenOCD is requested, then run it in a subprocess here, with an RAII
    // handle to ensure that it's killed before the program exits.
    struct OpenOcdRunner(std::process::Child);
    impl Drop for OpenOcdRunner {
        fn drop(&mut self) {
            self.0.kill().expect("Could not kill `openocd`")
        }
    }
    //TODO feel like this should just call to humility openocd
    let _openocd = if subargs.run_openocd {
        hubris
            .extract_file_to(
                "debug/openocd.cfg",
                &work_dir.path().join("openocd.cfg"),
            )
            .context("openocd config missing. Is your Hubris build too old?")?;
        let mut cmd = Command::new(
            subargs.openocd.unwrap_or_else(|| "openocd".to_string()),
        );
        cmd.arg("-f").arg("openocd.cfg");
        if let Some(serial) = serial {
            cmd.arg("-c")
                .arg("interface hla")
                .arg("-c")
                .arg(format!("hla_serial {}", serial));
        }
        cmd.current_dir(work_dir.path());
        cmd.stdin(Stdio::piped());
        Some(OpenOcdRunner(cmd.spawn().context("Could not start `openocd`")?))
    } else {
        None
    };

    // Alright, here's where it gets awkward.  We are either
    // - Running OpenOCD, launched by the block above
    // - Running OpenOCD in a separate terminal, through humility openocd
    //   or manually
    // - Running PyOCD in a separate terminal
    //
    // If we aren't loading new firmware, then we want to check that the
    // running firmware matches the image.  However, we can't use
    // humility_cmd::attach like normal, because that's not compatible with
    // PyOCD (which doesn't expose the TCL port).
    //
    // Instead, we fall back to the one interface that we _know_ these three
    // cases have in common: GDB.  Also, GDB is _terrible_: `print/x` doesn't
    // work if we're attached to the target and have all of our sections
    // loaded.
    if !subargs.load && !subargs.skip_check {
        let mut cmd = Command::new(gdb_cmd);
        let image_id_addr = hubris.image_id_addr().unwrap();
        let image_id = hubris.image_id().unwrap();
        cmd.arg("-q")
            .arg("-ex")
            .arg(format!("target extended-remote :{}", subargs.port))
            .arg("-ex")
            .arg(format!(
                "dump binary memory image_id {} {}",
                image_id_addr,
                image_id_addr as usize + image_id.len(),
            ))
            .arg("-ex")
            .arg("set confirm off")
            .arg("-ex")
            .arg("disconnect")
            .arg("-ex")
            .arg("quit");
        cmd.current_dir(work_dir.path());
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("could not get image_id, see output for details");
        }
        let image_id_actual = std::fs::read(work_dir.path().join("image_id"))?;
        if image_id_actual != image_id {
            bail!(
                "Invalid image ID: expected {:?}, got {:?}",
                image_id,
                image_id_actual
            );
        }
    }

    let mut cmd = Command::new(gdb_cmd);
    cmd.arg("-q")
        .arg("-x")
        .arg("script.gdb")
        .arg("-ex")
        .arg(format!("target extended-remote :{}", subargs.port))
        // print demangled symbols
        .arg("-ex")
        .arg("set print asm-demangle on")
        // prevent infiniti backtrace loops
        .arg("-ex")
        .arg("set backtrace limit 32")
        // most debugging will appreciate hex prints
        .arg("-ex")
        .arg("set radix 16");

    if subargs.load {
        // start the process but immediately halt the processor
        cmd.arg("-ex")
            .arg("load")
            .arg("-ex")
            .arg("stepi")
            .arg("-ex")
            .arg("set radix 16");
    }
    cmd.arg("final.elf");
    cmd.current_dir(work_dir.path());

    // Run GDB, ignoring Ctrl-C (so it can handle them)
    ctrlc::set_handler(|| {}).expect("Error setting Ctrl-C handler");
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("command failed, see output for details");
    }
    Ok(())
}

pub fn init() -> (HumilityCmd, ClapCommand<'static>) {
    (
        HumilityCmd::Unattached {
            name: "gdb",
            archive: Archive::Required,
            run: gdb,
        },
        GdbArgs::command(),
    )
}
