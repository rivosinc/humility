// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<()> {
    use cargo_metadata::MetadataCommand;
    let mut cmds = BTreeMap::new();

    let metadata =
        MetadataCommand::new().manifest_path("./Cargo.toml").exec().unwrap();

    for id in &metadata.workspace_members {
        let package =
            metadata.packages.iter().find(|p| &p.id == id).unwrap().clone();

        if let Some(cmd) = package.name.strip_prefix("humility-cmd-") {
            let description = match package.description {
                Some(description) => description,
                None => {
                    bail!("{} is missing \"description\" in manifest", cmd);
                }
            };

            cmds.insert(cmd.to_string(), (description, package.manifest_path));
        }
    }

    // we insert repl into the list of commands manually, since it is not its own package in the workspace
    cmds.insert(
        String::from("repl"),
        (String::from("read, eval, print, loop"), PathBuf::new()),
    );

    let out_dir = env::var("OUT_DIR")?;
    let dest_path = Path::new(&out_dir).join("docs.rs");
    let mut output = File::create(&dest_path)?;

    write!(
        output,
        r##"
fn cmd_docs(lookup: &str) -> Option<&'static str> {{
    let mut m = HashMap::new();
"##
    )?;

    for (cmd, (_, path)) in &cmds {
        // because we have an empty PathBuf in path, none of the below will work
        // for the repl. This means that we instead detect if we're working with
        // the repl subcommand, and if so, manually write out the docs for it,
        // and then continue looping for everything else
        if cmd == "repl" {
            write!(output, "        m.insert(\"repl\", r##\"")?;
            write!(output, "\n\n`humility repl` is an interactive prompt that you can use with humility.
This allows you to run several commands in succession without needing to type in some core settings
over and over again.

`humility repl` takes the same top level arguments as any other subcommand, and will remember them
inside of the prompt. For example:

```
$ humility -a ../path/to/hubris/archive.zip repl
humility: attached via ST-Link V2-1
Welcome to the humility REPL! Try out some subcommands, or 'quit' to quit!
humility> tasks
system time = 7209837
ID TASK                 GEN PRI STATE
 0 jefe                   0   0 recv, notif: bit0 bit1(T+63)
 1 rcc_driver             0   1 recv
 2 usart_driver           0   2 RUNNING
 3 user_leds              0   2 recv
 4 ping               47524   4 wait: reply from usart_driver/gen0
 5 pong                   0   3 recv, notif: bit0(T+163)
 6 hiffy                  0   3 notif: bit31(T+121)
 7 idle                   0   5 ready

humility> tasks
system time = 7212972
ID TASK                 GEN PRI STATE
 0 jefe                   0   0 recv, notif: bit0 bit1(T+28)
 1 rcc_driver             0   1 recv
 2 usart_driver           0   2 recv, notif: bit0(irq38)
 3 user_leds              0   2 recv
 4 ping               47544   4 RUNNING
 5 pong                   0   3 recv, notif: bit0(T+28)
 6 hiffy                  0   3 notif: bit31(T+252)
 7 idle                   0   5 ready

humility> quit
Quitting!
```

As you can see, we can run the `tasks` subcommand twice, without passing our
archive each time. In the output above, you can see the ping task faulting
in the background; your code is still running in the background while you
use the repl!

Finally, as you can see, `quit` will quit the repl. There is also a `history`
command, which will show you recent commands you've put into the prompt.

The repl is still very early days! We'll be adding more features in the future.
            ")?;
            write!(output, "\n\n")?;
            writeln!(output, "\"##);\n")?;

            continue;
        }
        println!(
            "cargo:rerun-if-changed={}",
            path.parent().unwrap().join("src/lib.rs").display()
        );

        let mut gencmd = Command::new("cargo");
        gencmd.arg("readme");
        gencmd.arg("--no-title");
        gencmd.arg("-r");
        gencmd.arg(path.parent().unwrap());

        let contents = gencmd.output()?;

        if !contents.status.success() {
            bail!(
                "\"cargo readme\" command failed for {}: {:?}; \
                have you run \"cargo install cargo-readme\"?",
                cmd,
                contents
            );
        }

        write!(output, "        m.insert(\"{}\", r##\"", cmd)?;

        let header = format!("### `humility {}`\n", cmd);

        if contents.stdout.len() == 1 {
            output.write_all(&header.as_bytes()[1..])?;
            writeln!(
                output,
                r##"
Welp, no additional documentation for {} -- but there obviously should be!
Mind opening an issue on that if one isn't open already?
"##,
                cmd
            )?;
        } else {
            if !contents.stdout.starts_with(header.as_bytes()) {
                bail!("malformed documentation for {}", cmd);
            }

            output.write_all(&contents.stdout[1..])?;

            //
            // If we don't end on a blank line, insert one.
            //
            if !contents.stdout.ends_with("\n\n".as_bytes()) {
                writeln!(output)?;
            }
        }

        writeln!(output, "\"##);\n")?;
    }

    writeln!(
        output,
        r##"

    m.get(lookup).as_ref().map(|result| result as _)
}}

fn docs() -> &'static str {{
    r##""##
    )?;

    let root = metadata.workspace_root;
    let input = std::fs::read(root.join("README.md.in"))?;

    output.write_all(&input)?;

    writeln!(
        output,
        r##"## Commands

Run `humility doc` with the specified command name for details on each command.
"##
    )?;

    for (cmd, (description, _)) in &cmds {
        writeln!(output, "- `humility {}`: {}", cmd, description)?;
    }

    writeln!(output, "\n\"##\n}}")?;

    Ok(())
}
