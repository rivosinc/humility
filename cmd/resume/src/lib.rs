// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility resume`
//!
//! `humility resume` will resume the core using the debug pin
//!

use anyhow::Result;
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility::core::Core;
use humility::hubris::*;
use humility_cmd::{Archive, Attach, Command, Run, Validate};

#[derive(Parser, Debug)]
#[clap(name = "resume", about = env!("CARGO_PKG_DESCRIPTION"))]
struct ResumeArgs {}

fn resume(
    _hubris: &HubrisArchive,
    core: &mut dyn Core,
    _subargs: &[String],
) -> Result<()> {
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
            validate: Validate::Match,
            run: Run::Subargs(resume),
        },
        ResumeArgs::command(),
    )
}
