// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility resume`
//!
//! `humility resume` will resume the core using the debug interface
//!

use anyhow::Result;
use clap::{CommandFactory, Parser};
use humility_cli::ExecutionContext;
use humility_cmd::{Archive, Attach, Command, CommandKind, Validate};

#[derive(Parser, Debug)]
#[clap(name = "resume", about = env!("CARGO_PKG_DESCRIPTION"))]
struct ResumeArgs {}

fn resume(context: &mut ExecutionContext) -> Result<()> {
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

pub fn init() -> Command {
    Command {
        app: ResumeArgs::command(),
        name: "resume",
        run: resume,
        kind: CommandKind::Attached {
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::None,
        },
    }
}
