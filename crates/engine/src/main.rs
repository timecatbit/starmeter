//! One run of Starmeter: the menu for SwiftBar, a background read, an action, the Claude status line, or a request
//! from an app. The SwiftBar entry script loads config.local.sh into the environment first.

use std::io::Write;

use starmeter_core::runtime::Runtime;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("--version") {
        println!("{}", starmeter_core::VERSION);
        return;
    }
    let mut runtime = Runtime::from_process();
    let status = runtime.main(&arguments);
    let _ = std::io::stdout().flush();
    std::process::exit(status);
}
