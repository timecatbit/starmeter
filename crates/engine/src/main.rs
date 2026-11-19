//! The program every host runs: the SwiftBar entry execs it, and the apps call it for refreshes and actions.
//! While Starmeter 6 is being ported only --version works; everything else exits with UNPORTED so the whole-run
//! tests skip it instead of failing.

/// sysexits' EX_CONFIG: the tests read this exit status as "not ported yet".
const UNPORTED: i32 = 78;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("--version") {
        println!("{}", starmeter_core::VERSION);
        return;
    }
    eprintln!("starmeter-engine {}: not ported yet", arguments.join(" "));
    std::process::exit(UNPORTED);
}
