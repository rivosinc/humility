// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility log`
//!
//! `humility log` reads and displays any Hubris string buffers (as created
//! via the `stringbuf!` macro in the Hubris `ringbuf` crate).  e.g.:
//!
//! ```console
//! % humility log -m LOG_RINGBUF
//! humility: attached via J-Link
//! humility: stringbuf ringbuf::stringbuf::LOG__STRINGBUF in jefe:
//! 5532: Task #2 Illegal instruction
//! 5535: Task #2 Memory fault at address 0x0
//! 5537: Task #2 Illegal instruction
//! ...
//! ```
//!
//! Use `-m` or `--monitor` to continuously monitor the buffer, otherwise will just print the
//! current log and exit.  When in monitor mode, the time (ms) between scanning for new log entries
//! can be changed with '-d' or '--delay', it defaults to 100ms.
//!
//! If an argument is provided, only string buffers that have a name that
//! contains the argument as a substring, or are in a task that contains
//! the argument as a substring will be displayed.  
//!

use anyhow::{bail, Result};
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility::cli::Subcommand;
use humility::core::Core;
use humility::hubris::*;
use humility::reflect::{self, Load, Value};
use humility_cmd::doppel::{StaticCell, Stringbuf};
use humility_cmd::{Archive, Attach, Command, Validate};
use std::str;
use std::thread;
use std::time;

#[derive(Parser, Debug)]
#[clap(name = "stringbuf", about = env!("CARGO_PKG_DESCRIPTION"))]
struct StringbufArgs {
    /// list all stringbufs
    #[clap(long, short)]
    list: bool,
    /// continously update the log
    #[clap(long, short, conflicts_with = "list")]
    monitor: bool,
    /// how long to delay in ms between polling for new log entries
    #[clap(
        long,
        short,
        default_value = "100",
        requires = "monitor",
        conflicts_with = "list"
    )]
    delay: u64,
    /// print only a single stringbuffer by substring of name
    #[clap(conflicts_with = "list")]
    name: Option<String>,
}

fn load_stringbuf(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    definition: &HubrisStruct,
    ringbuf_var: &HubrisVariable,
) -> Stringbuf {
    let mut buf: Vec<u8> = vec![];
    buf.resize_with(ringbuf_var.size, Default::default);
    // Load the stringbuf data from buf
    core.op_start().unwrap();
    core.read_8(ringbuf_var.addr, buf.as_mut_slice()).unwrap();
    core.op_done().unwrap();

    // use the buf to create a stringbuf struct
    let ringbuf_val: Value = Value::Struct(
        reflect::load_struct(hubris, buf.as_mut_slice(), definition, 0)
            .unwrap(),
    );
    let cell: StaticCell = StaticCell::from_value(&ringbuf_val).unwrap();
    Stringbuf::from_value(&cell.cell.value).unwrap()
}

fn stringbuf_read(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    definition: &HubrisStruct,
    ringbuf_var: &HubrisVariable,
    prev_last_idx: usize,
) -> Result<(String, usize)> {
    let mut log_msg: String = "".to_owned();

    // load the stringbuf from hubris into a corresponding local struct
    let mut stringbuf: Stringbuf =
        load_stringbuf(hubris, core, definition, ringbuf_var);

    // extract the log itself as a [u8]
    let buffer = stringbuf.buffer.as_mut_slice();
    // the last written element in the log
    let last = stringbuf.last.unwrap() as usize;

    let start_read_idx = (prev_last_idx + 1) % buffer.len();
    // now circular rotate our buffer so it starts with the new characters
    buffer.rotate_left(start_read_idx as usize);

    //
    // rotate the last index to match the buffer
    //
    // we add buffer.len to prevent overflow, it will be "removed" when
    // we take remainder later
    let rot_last = (last + buffer.len()) - start_read_idx;
    // take the modulus, ("%" is remainder in rust, but since we already offset by `buffer.len()`
    // it is guaranteed to be > 0 still.
    let rot_last = rot_last % buffer.len();

    for i in 0..rot_last {
        match buffer[i as usize] as char {
            // don't print the carriage return or null character
            '\0' | '\r' => continue,
            c => log_msg += &format!("{}", c as char),
        }
    }

    Ok((log_msg, last))
}
fn stringbuf_monitor(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    definition: &HubrisStruct,
    ringbuf_var: &HubrisVariable,
    delay: u64,
) -> Result<()> {
    //
    // TODO
    // set ctrl c handler before looping
    // this will ensure that op_done is always called even if interrupted
    // see jira https://rivosinc.atlassian.net/browse/SW-440
    //
    // represents the last seen values from the string buffer
    let mut prev_last_idx = 0;
    let mut last_log: String = "".to_owned();
    loop {
        let (log, last) = stringbuf_read(
            hubris,
            core,
            definition,
            ringbuf_var,
            prev_last_idx,
        )?;

        // don't print a line if we have already seen it.
        if last == prev_last_idx && log == last_log {
            continue;
        }

        print!("{}", log);

        // update state for last buffer
        prev_last_idx = last;
        last_log = log;

        // this delay is needed so we are not constantly halting the core
        thread::sleep(time::Duration::from_millis(delay));
    }
}

fn taskname<'a>(
    hubris: &'a HubrisArchive,
    variable: &'a HubrisVariable,
) -> Result<&'a str> {
    Ok(&hubris.lookup_module(HubrisTask::from(variable.goff))?.name)
}

// this allow is meant for the header println! in the body but you cannot apply
// an attribute to a macro invoction, so we have to put it here instead.
#[allow(clippy::print_literal)]
fn stringbuf(context: &mut humility::ExecutionContext) -> Result<()> {
    let hubris = context.archive.as_ref().unwrap();
    let core = &mut **context.core.as_mut().unwrap();
    let Subcommand::Other(subargs) = context.cli.cmd.as_ref().unwrap();
    let subargs = StringbufArgs::try_parse_from(subargs)?;

    let mut ringbufs = vec![];

    for v in hubris.qualified_variables() {
        if let Some(ref name) = subargs.name {
            if v.0.eq(name)
                || (v.0.ends_with("_STRINGBUF")
                    && (v.0.contains(name)
                        || taskname(hubris, v.1)?.contains(name)))
            {
                ringbufs.push(v);
            }
        } else if v.0.ends_with("_STRINGBUF") {
            ringbufs.push(v);
        }
    }

    if ringbufs.is_empty() {
        if let Some(name) = subargs.name {
            bail!("no ring buffer name contains \"{}\" (-l to list)", name);
        } else {
            bail!("no ring buffers found");
        }
    }

    if subargs.monitor && ringbufs.len() != 1 {
        if let Some(name) = subargs.name {
            bail!(
                "\"{}\" matched more than one stringbuf (-l to list all)",
                name
            );
        } else {
            bail!("found more than one stringbuf, please specify a name");
        }
    }

    ringbufs.sort();

    if subargs.list {
        println!("{:18} {:<30} {:<10} {}", "MODULE", "BUFFER", "ADDR", "SIZE");

        for v in ringbufs {
            let t = taskname(hubris, v.1)?;

            println!("{:18} {:<30} 0x{:08x} {:<}", t, v.0, v.1.addr, v.1.size);
        }

        return Ok(());
    }

    for v in ringbufs {
        // Try not to use `?` here, because it causes one bad ringbuf to make
        // them all unavailable.
        println!(
            "humility: stringbuf {} in {}:",
            v.0,
            taskname(hubris, v.1).unwrap_or("???")
        );
        if let Ok(def) = hubris.lookup_struct(v.1.goff) {
            if subargs.monitor {
                if let Err(e) =
                    stringbuf_monitor(hubris, core, def, v.1, subargs.delay)
                {
                    humility::msg!("stringbuf monitor cancelled: {}", e);
                }
            } else {
                // this first read is just to get the last entry so we can print the buffer in
                // order
                let last = load_stringbuf(hubris, core, def, v.1).last.unwrap();
                // now reuse that last to ensure we read the whole buffer in the correct order
                let (log, _last) =
                    stringbuf_read(hubris, core, def, v.1, last as usize)?;
                print!("{}", log);
            }
        } else {
            humility::msg!("could not look up type: {:?}", v.1.goff);
        }
    }

    Ok(())
}

pub fn init() -> (Command, ClapCommand<'static>) {
    (
        Command::Attached {
            name: "log",
            archive: Archive::Required,
            attach: Attach::Any,
            validate: Validate::Match,
            run: stringbuf,
        },
        StringbufArgs::command(),
    )
}
