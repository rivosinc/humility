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
//! humility: ring buffer task_jefe::LOG_RINGBUF in jefe:
//! instruction
//! Task #2 Memory fault at address 0x0
//! Task #2 Illegal instruction
//! Task #2 Memory fault at address 0x0
//! Task #2 Illegal instruction
//! Task #2 Memory fault at address 0x0
//! Task #2 Illegal instruction
//! ...
//! ```
//!
//! Use `-m` or `--monitor` to continuously monitor the buffer, otherwise will just print the
//! current log and exit.
//!
//! If an argument is provided, only string buffers that have a name that
//! contains the argument as a substring, or are in a task that contains
//! the argument as a substring will be displayed.  
//!
//! See the [`ringbuf`
//! documentation](https://github.com/oxidecomputer/hubris/blob/master/lib/ringbuf/src/lib.rs) for more details.

use anyhow::{bail, Result};
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility::core::Core;
use humility::hubris::*;
use humility::reflect::{self, Load, Value};
use humility_cmd::doppel::{StaticCell, Stringbuf};
use humility_cmd::{Archive, Attach, Command, Run, Validate};
use std::collections::BTreeMap;
use std::iter::zip;
use std::num::ParseIntError;
use std::str;
use std::thread;
use std::time;

#[derive(Parser, Debug)]
#[clap(name = "stringbuf", about = env!("CARGO_PKG_DESCRIPTION"))]
struct StringbufArgs {
    /// list variables
    #[clap(long, short)]
    list: bool,
    /// continously update the log
    #[clap(long, short)]
    monitor: bool,
    /// how long to delay between polling for new log entries
    #[clap(long, short, default_value = "100")]
    delay: u64,
    /// print only a single ringbuffer by substring of name
    #[clap(conflicts_with = "list")]
    name: Option<String>,
}

///
/// Extract the timestamp from the line
///
fn extract_timestamp(l: &str) -> Result<u64, ParseIntError> {
    let mut line = l.split(':');
    // the first element before the ':' should be the timestamp
    let hopefully_number_part = line.nth(0).unwrap();
    let timestamp = hopefully_number_part.parse();
    return timestamp;
}

fn stringbuf_monitor(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    definition: &HubrisStruct,
    ringbuf_var: &HubrisVariable,
    delay: Option<u64>,
    once: bool,
) -> Result<()> {
    let mut buf: Vec<u8> = vec![];
    buf.resize_with(ringbuf_var.size, Default::default);

    let mut update_stringbuf = || -> Stringbuf {
        let _info = core.op_start().unwrap();
        core.read_8(ringbuf_var.addr, buf.as_mut_slice()).unwrap();
        core.op_done().unwrap();

        // There are two possible shapes of ringbufs, depending on the age of the
        // firmware.
        // - Raw Ringbuf that is not wrapped by anything.
        // - Safe Ringbuf that is inside a StaticCell.
        //
        // Here we will attempt to handle them both -- first raw, then fallback.
        let ringbuf_val: Value = Value::Struct(
            reflect::load_struct(hubris, &buf, definition, 0).unwrap(),
        );

        Stringbuf::from_value(&ringbuf_val)
            .or_else(|_e| {
                let cell: StaticCell =
                    StaticCell::from_value(&ringbuf_val).unwrap();
                Stringbuf::from_value(&cell.cell.value)
            })
            .unwrap()
    };

    // use a BTree to keep a sorted hashmap of all the log entries,  this prevents reprinting log
    // lines as well as keeping them ordered.
    let mut full_log = BTreeMap::new();

    //
    //TODO
    // set ctrl c handler before looping
    // this will ensure that op_done is always called even if interrupted
    //

    loop {
        //
        // Read the log from the core and convert to a big string
        //
        let stringbuf = update_stringbuf();
        let buffer = stringbuf.buffer.as_slice();
        let log = str::from_utf8(buffer);
        if let Err(_err) = log {
            println!("Invalid Log: {:?}", stringbuf);
        }

        //
        // split log file up into lines based on null delimeter,
        // also count the characters
        //
        let log = log.unwrap();
        let mut lines: Vec<&str> = log.split('\0').collect();
        let char_counts: Vec<usize> =
            lines.iter().map(|line| line.len()).collect();

        //
        // remove the line that is currently being overwritten
        //
        let last_idx: usize = stringbuf.last.unwrap() as usize;
        let last_char = buffer[last_idx];
        if last_char != 0 && last_char != b'\r' {
            let mut total_count = 0;
            for (char_count, (i, _line)) in
                zip(char_counts, lines.clone().iter().enumerate())
            {
                if total_count <= last_idx
                    && total_count + char_count > last_idx
                {
                    lines.swap_remove(i);
                    //println!("removed partial line: {}", line);
                    break;
                }
                total_count += char_count;
            }
        }

        for (i, l) in lines.iter().enumerate() {
            //
            // extract and verify timestamp
            //
            let timestamp = extract_timestamp(l);
            if let Err(_err) = timestamp {
                //println!("invalid Log Entry found: {:?}", l);
                continue;
            }
            let timestamp: u64 = timestamp.unwrap();

            //
            // Insert line into map
            //
            if !full_log.contains_key(&timestamp) {
                // the first line could contain a partial timestamp, so verify it starts with a
                // null character, otherwise skip
                if i == 0 && buffer[0] != 0 {
                    continue;
                }

                // line could have ended partially through another, so strip anything after `\r`
                let mut l = l.split('\r');
                let l = l.nth(0).unwrap().to_string();

                // if on the last chunk, may need to wrap around to the beginning of the buffer
                if i == lines.len() - 1
                    && buffer[0] != 0
                    && l.chars().last().unwrap() != '\n'
                {
                    // if wrapping need to strip the \r and anything after it on the wrapped line
                    let mut wrap = lines.as_slice()[0].split('\r');
                    let wrap = wrap.nth(0).unwrap();
                    let s = l + wrap;
                    full_log.insert(timestamp, (false, s));
                } else {
                    // any line but the last
                    full_log.insert(timestamp, (false, l));
                }
            }
        }

        // TODO
        // dont want the btree to grow indefinitly, so cull old entries.
        // The ringbuf contains at most ringbuf.len lines so  `full_log` just needs to be able to
        // contain all of them to prevent repeats from being printed
        //while full_log.len() > buffer.len() {
        //    full_log.pop_first();
        //}

        for (_key, (printed, line)) in full_log.iter_mut() {
            if !*printed {
                print!("{}", line);
                *printed = true;
            }
        }

        if once {
            break;
        }

        thread::sleep(time::Duration::from_millis(delay.unwrap()));
    }

    Ok(())
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
fn stringbuf(
    hubris: &HubrisArchive,
    core: &mut dyn Core,
    subargs: &[String],
) -> Result<()> {
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

    if subargs.monitor {
        if ringbufs.len() != 1 {
            if let Some(name) = subargs.name {
                bail!(
                    "\"{}\" matched more than one ring buffer (-l to list all)",
                    name
                );
            } else {
                bail!("found more than one ring buffer, please specify a name");
            }
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
            "humility: ring buffer {} in {}:",
            v.0,
            taskname(hubris, v.1).unwrap_or("???")
        );
        if let Ok(def) = hubris.lookup_struct(v.1.goff) {
            if subargs.monitor {
                if let Err(e) = stringbuf_monitor(
                    hubris,
                    core,
                    def,
                    v.1,
                    Some(subargs.delay),
                    false,
                ) {
                    humility::msg!("ringbuf monitor cancelled: {}", e);
                }
            } else {
                if let Err(e) =
                    stringbuf_monitor(hubris, core, def, v.1, None, true)
                {
                    humility::msg!("ringbuf monitor cancelled: {}", e);
                }
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
            run: Run::Subargs(stringbuf),
        },
        StringbufArgs::command(),
    )
}
