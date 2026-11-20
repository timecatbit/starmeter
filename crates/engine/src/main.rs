//! One run of Starmeter: the menu for SwiftBar, a background read, an action, the Claude status line, or a request
//! from an app. The SwiftBar entry script loads config.local.sh into the environment first. Run under the name
//! `starmeter` (a link or a copy, as the command-line packages install it), or as `starmeter-engine cli …`, it is the
//! command line.

mod cli;

use std::io::Write;
use std::path::Path;

use starmeter_core::runtime::Runtime;

/// Whether the program was started as `starmeter`: the command line's name, not the engine's.
fn named_starmeter(argv0: Option<&str>) -> bool {
    argv0.and_then(|path| Path::new(path).file_stem()).and_then(|stem| stem.to_str()) == Some("starmeter")
}

fn main() {
    let mut arguments = std::env::args();
    let argv0 = arguments.next();
    let arguments: Vec<String> = arguments.collect();
    if named_starmeter(argv0.as_deref()) {
        let status = cli::main(&arguments);
        let _ = std::io::stdout().flush();
        std::process::exit(status);
    }
    match arguments.first().map(String::as_str) {
        Some("--version") => {
            println!("{}", starmeter_core::VERSION);
            return;
        }
        Some("cli") => {
            let status = cli::main(&arguments[1..]);
            let _ = std::io::stdout().flush();
            std::process::exit(status);
        }
        _ => {}
    }
    let mut runtime = Runtime::from_process();
    let status = runtime.main(&arguments);
    let _ = std::io::stdout().flush();
    std::process::exit(status);
}

#[cfg(test)]
mod tests {
    use super::named_starmeter;

    #[test]
    fn only_the_command_lines_name_starts_it() {
        assert!(named_starmeter(Some("starmeter")));
        assert!(named_starmeter(Some("/opt/homebrew/bin/starmeter")));
        assert!(named_starmeter(Some(r"C:\Starmeter\starmeter.exe")) || cfg!(not(windows)));
        assert!(!named_starmeter(Some("/Users/me/.opencodex-panel/bin/starmeter-engine")));
        assert!(!named_starmeter(Some("starmeter.30s.sh")));
        assert!(!named_starmeter(Some("Starmeter")));
        assert!(!named_starmeter(None));
    }
}
