// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "xraybox-vpn",
    platforms: [
        .iOS(.v17)
    ],
    products: [
        .library(
            name: "xraybox-vpn",
            type: .static,
            targets: ["xraybox-vpn"]
        )
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "xraybox-vpn",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources",
            linkerSettings: [
                .linkedFramework("NetworkExtension")
            ]
        )
    ]
)
