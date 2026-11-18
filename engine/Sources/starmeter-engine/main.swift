import Foundation
import StarmeterCore

// One run of Starmeter: the menu for SwiftBar, a background read, an action, or the Claude status line.
// The entry script loads config.local.sh into the environment first.
#if !os(Windows)
setvbuf(stdout, nil, _IOFBF, 1 << 16)
#endif
let runtime = Runtime()
let status = runtime.main(Array(CommandLine.arguments.dropFirst()))
#if !os(Windows)
fflush(stdout)
#endif
exit(status)
