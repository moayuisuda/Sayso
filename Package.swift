// swift-tools-version: 6.2
import PackageDescription

let swiftSettings: [SwiftSetting] = [.swiftLanguageMode(.v5)]

let package = Package(
    name: "Sayso",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "Sayso", targets: ["Sayso"]),
    ],
    targets: [
        // Rust core 静态库与 C 头文件，由 scripts/build-core.sh 生成。
        .binaryTarget(
            name: "SaysoCoreFFI",
            path: "build/SaysoCoreFFI.xcframework"
        ),
        // UniFFI 生成的 Swift 绑定（Sources/SaysoCore/SaysoCore.swift，由 scripts/build-core.sh 生成）。
        .target(
            name: "SaysoCore",
            dependencies: ["SaysoCoreFFI"],
            path: "Sources/SaysoCore",
            swiftSettings: swiftSettings,
            linkerSettings: [.linkedFramework("Security"), .linkedFramework("CoreFoundation"), .linkedLibrary("iconv")]
        ),
        // macOS 宿主：平台适配层（录音、选区、写入、热键、朗读）与 SwiftUI 界面。
        .executableTarget(
            name: "Sayso",
            dependencies: ["SaysoCore"],
            path: "Sources/Sayso",
            swiftSettings: swiftSettings
        ),
    ]
)
