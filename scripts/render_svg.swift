// render_svg.swift <file.svg> <pixels> <out.png>
// Draws an SVG into a square PNG with AppKit: the menu bar star and the 1024-pixel source `tauri icon` packs.
import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 4, let image = NSImage(contentsOfFile: arguments[1]), let pixels = Int(arguments[2]),
      let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
                                    samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                    bytesPerRow: 0, bitsPerPixel: 0),
      let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
    FileHandle.standardError.write("usage: render_svg.swift <file.svg> <pixels> <out.png>\n".data(using: .utf8)!)
    exit(2)
}
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = context
image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels))
NSGraphicsContext.restoreGraphicsState()
guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
try png.write(to: URL(fileURLWithPath: arguments[3]))
