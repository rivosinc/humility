// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility halt`
//!
//! `humility halt` will halt the system using the debug interface
//!

use anyhow::Result;
use clap::{CommandFactory, Parser};
use humility_cli::ExecutionContext;
use humility_cmd::{Archive, Attach, Command, CommandKind, Validate};

#[derive(Parser, Debug)]
#[clap(name = "halt", about = env!("CARGO_PKG_DESCRIPTION"))]
struct HaltArgs {}

fn halt(context: &mut ExecutionContext) -> Result<()> {
    let core = &mut **context.core.as_mut().unwrap();
    let r = core.halt();

    if r.is_err() {
        humility::msg!(
            "There was an error when halting. \
            The chip may be in an unknown state!"
        );
        humility::msg!("Full error: {:x?}", r);
    } else {
        humility::msg!("core halted");
    }

    Ok(())
}

pub fn init() -> Command {
    Command {
        app: HaltArgs::command(),
        name: "halt",
        run: halt,
        kind: CommandKind::Attached {
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::None,
        },
    }
}
