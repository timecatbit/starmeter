// swift-tools-version:5.9
// The Starmeter data engine: one Swift code base for the macOS app, the SwiftBar plugin and the Windows tray.
// No dependencies to fetch; SQLite comes from the system.
import PackageDescription

let package = Package(
    name: "StarmeterEngine",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "StarmeterCore", targets: ["StarmeterCore"]),
        .executable(name: "starmeter-engine", targets: ["starmeter-engine"]),
    ],
    targets: [
        .systemLibrary(name: "CSQLite", path: "Sources/CSQLite"),
        .target(name: "StarmeterCore", dependencies: ["CSQLite"], linkerSettings: [
            .linkedLibrary("sqlite3", .when(platforms: [.macOS, .linux])),
            .linkedLibrary("winsqlite3", .when(platforms: [.windows])),
        ]),
        // The program every host runs: SwiftBar's entry, the standalone app's worker and the Windows tray.
        .executableTarget(name: "starmeter-engine", dependencies: ["StarmeterCore"]),
        // Answers one call per run on stdin, so the tests can hold the Swift engine against the Python one.
        .executableTarget(name: "starmeter-probe", dependencies: ["StarmeterCore"]),
    ]
)
