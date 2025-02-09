// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility halt`
//!
//! `humility halt` will halt the system using the debug interface
//!

use anyhow::Result;
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility_cmd::{Archive, Attach, Command, Validate};

#[derive(Parser, Debug)]
#[clap(name = "halt", about = env!("CARGO_PKG_DESCRIPTION"))]
struct HaltArgs {}

fn halt(context: &mut humility::ExecutionContext) -> Result<()> {
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

pub fn init() -> (Command, ClapCommand<'static>) {
    (
        Command::Attached {
            name: "halt",
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::None,
            run: halt,
        },
        HaltArgs::command(),
    )
}
