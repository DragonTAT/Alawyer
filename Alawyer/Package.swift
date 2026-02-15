// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Alawyer",
    defaultLocalization: "zh-Hans",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "Alawyer", targets: ["Alawyer"]),
    ],
    targets: [
        .systemLibrary(
            name: "alawyer_coreFFI",
            path: "Sources/Generated/ffi"
        ),
        .target(
            name: "CoreBindings",
            dependencies: ["alawyer_coreFFI"],
            path: "Sources/Generated",
            exclude: ["ffi", "alawyer_coreFFI.h", "alawyer_coreFFI.modulemap"],
            sources: ["alawyer_core.swift"],
            swiftSettings: [
                .unsafeFlags(["-Xfrontend", "-strict-concurrency=minimal"]),
            ],
            linkerSettings: [
                .unsafeFlags(["-L../alawyer-core/target/debug", "-lalawyer_core"]),
            ]
        ),
        .executableTarget(
            name: "Alawyer",
            dependencies: ["CoreBindings"],
            path: "Sources",
            exclude: ["Generated"],
            resources: [
                .copy("Support/SeedKB"),
            ],
            linkerSettings: [
                .linkedFramework("Security"),
            ]
        ),
        .testTarget(
            name: "AlawyerTests",
            dependencies: ["CoreBindings"],
            path: "Tests"
        ),
    ]
    ,
    swiftLanguageModes: [.v5]
)
