#!/usr/bin/env swift
// 从 Resources/AppIcon.svg 生成各平台图标：
//   Resources/AppIcon.icns          macOS 应用图标（保留系统网格留白）
//   windows/Sayso/Assets/Sayso.ico  Windows 应用与托盘图标（只裁取主体）
//   Resources/MenuBarIcon.png       macOS 菜单栏模板图标（主体实心、字形镂空的单色蒙版）
// 用法：swift scripts/render-icons.swift
import AppKit

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let source = root.appendingPathComponent("Resources/AppIcon.svg")

guard let svgText = try? String(contentsOf: source, encoding: .utf8), let svg = NSImage(data: Data(svgText.utf8)) else {
    fatalError("无法读取 \(source.path)")
}

/// SVG 画布尺寸与 macOS 网格中主体所在区域（与 AppIcon.svg 中的 rect 保持一致）。
let canvas = NSRect(x: 0, y: 0, width: 1024, height: 1024)
let body = NSRect(x: 100, y: 100, width: 824, height: 824)

/// 将 SVG 的 `region` 区域栅格化为 `size`×`size` 的位图。
func bitmap(_ image: NSImage, size: Int, region: NSRect) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.size = canvas.size
    image.draw(in: NSRect(x: 0, y: 0, width: size, height: size), from: region, operation: .copy, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

func png(_ size: Int, region: NSRect) -> Data {
    bitmap(svg, size: size, region: region).representation(using: .png, properties: [:])!
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

/// macOS 菜单栏：把源 SVG 的主体涂黑、字形与句点涂白、去掉高光，栅格化后按白色程度镂空，
/// 得到「主体实心、字形透明」的单色蒙版。系统按菜单栏明暗自动着色。
func writeMenuBarTemplate(to output: URL) throws {
    var text = svgText
    for (color, replacement) in [
        ("fill=\"url(#body)\"", "fill=\"#000000\""),
        ("stroke-opacity=\"0.12\"", "stroke-opacity=\"0\""),
        ("fill=\"#9FA8DA\"", "fill=\"#FFFFFF\""),
    ] {
        guard text.contains(color) else { fatalError("AppIcon.svg 中找不到 \(color)") }
        text = text.replacingOccurrences(of: color, with: replacement)
    }
    guard text.contains("fill=\"#F4F1E8\"") else { fatalError("AppIcon.svg 中找不到字形颜色") }
    text = text.replacingOccurrences(of: "fill=\"#F4F1E8\"", with: "fill=\"#FFFFFF\"")
    guard let mask = NSImage(data: Data(text.utf8)) else { fatalError("无法生成菜单栏蒙版") }

    // 64px 供 16pt 菜单栏图标在 2x/3x 屏幕上缩放使用。
    let rep = bitmap(mask, size: 64, region: body)
    for y in 0..<rep.pixelsHigh {
        for x in 0..<rep.pixelsWide {
            let pixel = rep.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
            // 黑白之间的抗锯齿过渡按白色程度线性镂空。
            let alpha = pixel.alphaComponent * (1 - pixel.redComponent)
            rep.setColor(NSColor(deviceRed: 0, green: 0, blue: 0, alpha: alpha), atX: x, y: y)
        }
    }
    try rep.representation(using: .png, properties: [:])!.write(to: output)
}

try writeICNS(to: root.appendingPathComponent("Resources/AppIcon.icns"))
try writeICO(to: root.appendingPathComponent("windows/Sayso/Assets/Sayso.ico"))
try writeMenuBarTemplate(to: root.appendingPathComponent("Resources/MenuBarIcon.png"))
print("Rendered AppIcon.icns, Sayso.ico and MenuBarIcon.png")
