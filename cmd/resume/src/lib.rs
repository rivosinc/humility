// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility resume`
//!
//! `humility resume` will resume the core using the debug interface
//!

use anyhow::Result;
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility_cmd::{Archive, Attach, Command, Validate};

#[derive(Parser, Debug)]
#[clap(name = "resume", about = env!("CARGO_PKG_DESCRIPTION"))]
struct ResumeArgs {}

fn resume(context: &mut humility::ExecutionContext) -> Result<()> {
    let core = &mut **context.core.as_mut().unwrap();
    let r = core.run();

    if r.is_err() {
        humility::msg!(
            "There was an error when running. \
            The chip may be in an unknown state!"
        );
        humility::msg!("Full error: {:x?}", r);
    } else {
        humility::msg!("core resumed");
    }

    Ok(())
}

pub fn init() -> (Command, ClapCommand<'static>) {
    (
        Command::Attached {
            name: "resume",
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::None,
            run: resume,
        },
        ResumeArgs::command(),
    )
}
