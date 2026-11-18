// make_icon.swift <icon.svg> <small.svg> <folder.iconset>
// Renders the app icon at every size an .icns holds; iconutil then packs the folder.
// 32 pixels and below use the small drawing, whose fewer, larger dots stay distinct.
import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 4, let icon = NSImage(contentsOfFile: arguments[1]), let small = NSImage(contentsOfFile: arguments[2]) else {
    FileHandle.standardError.write("usage: make_icon.swift <icon.svg> <small.svg> <folder.iconset>\n".data(using: .utf8)!)
    exit(2)
}
let folder = URL(fileURLWithPath: arguments[3])
try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
                                            samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                            bytesPerRow: 0, bitsPerPixel: 0),
              let context = NSGraphicsContext(bitmapImageRep: bitmap) else { exit(1) }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        (pixels <= 32 ? small : icon).draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels))
        NSGraphicsContext.restoreGraphicsState()
        let name = "icon_\(points)x\(points)" + (scale == 2 ? "@2x" : "") + ".png"
        guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
        try png.write(to: folder.appendingPathComponent(name))
    }
}
