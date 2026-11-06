// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "FastMode",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(name: "FastMode", path: "Sources/FastMode", swiftSettings: [.swiftLanguageMode(.v5)])
    ]
)
