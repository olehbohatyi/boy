use std::process::ExitCode;

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

    match args.first().map(String::as_str) {
        None => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(cmd @ ("i" | "r" | "l" | "s" | "u")) => {
            eprintln!("boy: `{cmd}` is not implemented yet");
            ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("boy: unknown command `{other}`\n");
            eprint!("{HELP}");
            ExitCode::from(2)
        }
    }
}
