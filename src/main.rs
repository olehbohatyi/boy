mod defs;
mod github;
mod home;
mod install;
mod lock;
mod sums;

use std::process::ExitCode;

use anyhow::{Result, bail};

const HELP: &str = concat!(
    "boy ",
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "  i <name>[@version]  install\n",
    "  r <name>            remove\n",
    "  l                   list installed\n",
    "  s <query>           search\n",
    "  u [name]            update\n",
);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        [] => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        [cmd @ ("i" | "r" | "l" | "s" | "u"), rest @ ..] => match run(cmd, rest) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("boy: {e:#}");
                ExitCode::FAILURE
            }
        },
        [other, ..] => {
            eprintln!("boy: unknown command `{other}`\n");
            eprint!("{HELP}");
            ExitCode::from(2)
        }
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let home = home::Home::discover()?;
    match (cmd, args) {
        ("i", [name]) if name.contains('@') => {
            bail!("installing a pinned version is not implemented yet")
        }
        ("i", [name]) => install::install(&home, name),
        ("i", _) => bail!("usage: boy i <name>"),
        _ => bail!("`{cmd}` is not implemented yet"),
    }
}
