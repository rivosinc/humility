// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::Result;
use clap::ArgMatches;
use cli::Cli;
use env::Environment;
use hubris::HubrisArchive;

pub mod arch;
pub mod cli;
pub mod core;
pub mod env;
pub mod hubris;
pub mod reflect;

#[macro_use]
extern crate num_derive;

/// Give CLI output for the user
///
/// This macro is intended to be used whenever producing secondary output to the
/// terminal for users to see. It is its own macro for two reasons:
///
/// 1. it will prepend "humility: " to the output
/// 2. it uses stderr rather than stdout
///
/// By using this macro, if we want to change these two things, it's much easier
/// than changing every single eprintln! in the codebase.
#[macro_export]
macro_rules! msg {
    ($fmt:expr) => ({
        eprintln!(concat!("humility: ", $fmt));
    });
    ($fmt:expr, $($arg:tt)*) => ({
        eprintln!(concat!("humility: ", $fmt), $($arg)*);
    });
}

pub struct ExecutionContext {
    pub core: Option<Box<dyn core::Core>>,
    pub history: Vec<String>,
    pub archive: Option<HubrisArchive>,
    pub environment: Option<Environment>,
    pub cli: Cli,
}

impl ExecutionContext {
    pub fn new(mut cli: Cli, m: &ArgMatches) -> Result<ExecutionContext> {
        let environment = match (&cli.environment, &cli.target) {
            (Some(ref env), Some(ref target)) => {
                let env = match Environment::from_file(env, target) {
                    Ok(e) => e,
                    Err(err) => {
                        eprintln!("failed to match environment: {:?}", err);
                        std::process::exit(1);
                    }
                };

                //
                // Cannot specify a dump/probe and also an environment and target
                //
                assert!(cli.dump.is_none());
                assert!(cli.probe.is_none());

                cli.probe = Some(env.probe.clone());

                //
                // If we have an archive on the command-line or in an environment
                // variable, we want ot prefer that over whatever is in the
                // environment file -- but we also want to warn the user about
                // what is going on.
                //
                if cli.archive.is_some() {
                    let msg = if m.occurrences_of("archive") == 1 {
                        "archive on command-line"
                    } else {
                        "archive in environment variable"
                    };

                    log::warn!(
                        "{} overriding archive in environment file",
                        msg
                    );
                } else {
                    cli.archive = Some(env.archive.clone());
                }

                Some(env)
            }

            (Some(ref env), None) => {
                if cli.list_targets {
                    let targets = match Environment::targets(env) {
                        Ok(targets) => targets,
                        Err(err) => {
                            eprintln!("failed to parse environment: {:?}", err);
                            std::process::exit(1);
                        }
                    };

                    if cli.terse {
                        println!(
                            "{}",
                            targets
                                .iter()
                                .map(|(t, _)| &**t)
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    } else {
                        println!("{:15} DESCRIPTION", "TARGET");

                        for (target, description) in &targets {
                            println!(
                                "{:15} {}",
                                target,
                                match description {
                                    Some(d) => d,
                                    None => "-",
                                }
                            );
                        }
                    }

                    std::process::exit(0);
                }

                if let Err(err) = Environment::validate(env) {
                    eprintln!("failed to parse environment: {:?}", err);
                    std::process::exit(1);
                }

                None
            }

            _ => None,
        };

        if cli.cmd.is_none() {
            eprintln!("humility failed: subcommand expected (--help to list)");
            std::process::exit(1);
        }

        //
        // Check to see if we have both a dump and an archive.  Because these
        // conflict with one another but because we allow both of them to be
        // set with an environment variable, we need to manually resolve this:
        // we want to allow an explicitly set value (that is, via the command
        // line) to win the conflict.
        //
        if cli.dump.is_some() && cli.archive.is_some() {
            match (
                m.occurrences_of("dump") == 1,
                m.occurrences_of("archive") == 1,
            ) {
                (true, true) => {
                    log::error!("cannot specify both a dump and an archive");
                    std::process::exit(1);
                }

                (false, false) => {
                    log::error!(
                        "both dump and archive have been set via environment \
                    variables; unset one of them, or use a command-line option \
                    to override"
                    );
                    std::process::exit(1);
                }

                (true, false) => {
                    log::warn!(
                    "dump on command-line overriding archive in environment"
                );
                    cli.archive = None;
                }

                (false, true) => {
                    log::warn!(
                    "archive on command-line overriding dump in environment"
                );
                    cli.dump = None;
                }
            }
        }

        Ok(ExecutionContext {
            core: None,
            history: Vec::new(),
            archive: None,
            environment,
            cli,
        })
    }
}
