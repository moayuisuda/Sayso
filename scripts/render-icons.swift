#!/usr/bin/env swift
// 从 Resources/AppIcon.svg 生成各平台图标：
//   Resources/AppIcon.icns          macOS 应用图标（保留系统网格留白）
//   windows/Sayso/Assets/Sayso.ico  Windows 应用与托盘图标（只裁取主体）
// 用法：swift scripts/render-icons.swift
import AppKit

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let source = root.appendingPathComponent("Resources/AppIcon.svg")

guard let svg = NSImage(contentsOf: source) else { fatalError("无法读取 \(source.path)") }

/// SVG 画布尺寸与 macOS 网格中主体所在区域（与 AppIcon.svg 中的 rect 保持一致）。
let canvas = NSRect(x: 0, y: 0, width: 1024, height: 1024)
let body = NSRect(x: 100, y: 100, width: 824, height: 824)

/// 将 SVG 的 `region` 区域栅格化为 `size`×`size` 的 PNG。
func png(_ size: Int, region: NSRect) -> Data {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    svg.size = canvas.size
    svg.draw(in: NSRect(x: 0, y: 0, width: size, height: size), from: region, operation: .copy, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

/// macOS：按 iconutil 约定写出 iconset 后打包为 icns。
func writeICNS(to output: URL) throws {
    let iconset = FileManager.default.temporaryDirectory.appendingPathComponent("AppIcon.iconset")
    try? FileManager.default.removeItem(at: iconset)
    try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
    for points in [16, 32, 128, 256, 512] {
        try png(points, region: canvas).write(to: iconset.appendingPathComponent("icon_\(points)x\(points).png"))
        try png(points * 2, region: canvas).write(to: iconset.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
    }
    let task = Process()
    task.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
    task.arguments = ["-c", "icns", iconset.path, "-o", output.path]
    try task.run()
    task.waitUntilExit()
    guard task.terminationStatus == 0 else { fatalError("iconutil 失败") }
    try FileManager.default.removeItem(at: iconset)
}

/// Windows：ICO 容器直接内嵌 PNG（Vista 起支持），按尺寸从小到大排列。
func writeICO(to output: URL) throws {
    let sizes = [16, 20, 24, 32, 40, 48, 64, 256]
    let images = sizes.map { png($0, region: body) }
    var data = Data()
    func append<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) } }

    append(UInt16(0)); append(UInt16(1)); append(UInt16(sizes.count))
    var offset = 6 + 16 * sizes.count
    for (size, image) in zip(sizes, images) {
        // 宽高字段为 0 表示 256。
        data.append(UInt8(size % 256)); data.append(UInt8(size % 256))
        data.append(0); data.append(0)
        append(UInt16(1)); append(UInt16(32))
        append(UInt32(image.count)); append(UInt32(offset))
        offset += image.count
    }
    images.forEach { data.append($0) }
    try data.write(to: output)
}

try writeICNS(to: root.appendingPathComponent("Resources/AppIcon.icns"))
try writeICO(to: root.appendingPathComponent("windows/Sayso/Assets/Sayso.ico"))
print("Rendered AppIcon.icns and Sayso.ico")
