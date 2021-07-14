/*
 * Copyright 2021 Oxide Computer Company
 */

use crate::cmd::*;
use crate::core::Core;
use crate::hiffy::*;
use crate::hubris::*;
use crate::Args;
use anyhow::Result;
use structopt::clap::App;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "hiffy", about = "manipulate HIF execution")]
struct HiffyArgs {
    /// sets timeout
    #[structopt(
        long, short, default_value = "5000", value_name = "timeout_ms",
        parse(try_from_str = parse_int::parse)
    )]
    timeout: u32,

    /// list HIF functions
    #[structopt(long, short)]
    list: bool,
}

fn hiffy(
    hubris: &mut HubrisArchive,
    _core: &mut dyn Core,
    _args: &Args,
    subargs: &Vec<String>,
) -> Result<()> {
    let subargs = HiffyArgs::from_iter_safe(subargs)?;

    if !subargs.list {
        bail!("expected -l");
    }

    let mut context = HiffyContext::new(hubris, subargs.timeout)?;

    let funcs = context.functions()?;
    let mut byid: Vec<Option<(&String, &HiffyFunction)>> = vec![];

    byid.resize(funcs.len(), None);

    for (name, func) in &funcs {
        let ndx = func.id.0 as usize;

        if ndx >= byid.len() {
            bail!("ID for function {} ({}) exceeds bounds", name, ndx);
        }

        if let Some((_, _)) = byid[ndx] {
            bail!("function ID {} has conflics", ndx);
        }

        byid[ndx] = Some((name, func));
    }

    println!("{:>3} {:30} {}", "ID", "FUNCTION", "#ARGS");

    for i in 0..byid.len() {
        if let Some((name, func)) = byid[i] {
            println!("{:3} {:30} {}", i, name, func.args.len());
        } else {
            bail!("missing function for ID {}", i);
        }
    }

    Ok(())
}

pub fn init<'a, 'b>() -> (Command, App<'a, 'b>) {
    (
        Command::Attached {
            name: "hiffy",
            archive: Archive::Required,
            attach: Attach::LiveOnly,
            validate: Validate::Booted,
            run: hiffy,
        },
        HiffyArgs::clap(),
    )
}
