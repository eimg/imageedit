// Render the Finder background at 1x and 2x using only the macOS SDK.
import AppKit

guard CommandLine.arguments.count == 2 else {
    fatalError("Usage: dmg-background <output-directory>")
}
let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let size = NSSize(width: 640, height: 400)
let image = NSImage(size: size)
let navy = NSColor(srgbRed: 0.13, green: 0.19, blue: 0.33, alpha: 1)

func text(_ value: String, y: CGFloat, font: NSFont, color: NSColor) {
    let style = NSMutableParagraphStyle()
    style.alignment = .center
    (value as NSString).draw(in: NSRect(x: 30, y: y, width: 580, height: 40),
        withAttributes: [.font: font, .foregroundColor: color, .paragraphStyle: style])
}

for scale in [1, 2] {
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil,
        pixelsWide: Int(size.width) * scale, pixelsHigh: Int(size.height) * scale,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    bitmap.size = size
    let context = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    NSGradient(starting: NSColor(srgbRed: 0.98, green: 0.99, blue: 1, alpha: 1),
        ending: NSColor(srgbRed: 0.88, green: 0.92, blue: 0.99, alpha: 1))!
        .draw(in: NSRect(origin: .zero, size: size), angle: -70)

    text("Install Lime", y: 306, font: .systemFont(ofSize: 28, weight: .semibold), color: navy)
    text("Drag Lime to Applications to install.", y: 274,
        font: .systemFont(ofSize: 16), color: navy.withAlphaComponent(0.8))

    // Finder icon centers are (170, 210) and (470, 210), measured from the top.
    let arrow = NSBezierPath()
    arrow.move(to: NSPoint(x: 286, y: 190))
    arrow.line(to: NSPoint(x: 352, y: 190))
    arrow.move(to: NSPoint(x: 337, y: 205))
    arrow.line(to: NSPoint(x: 352, y: 190))
    arrow.line(to: NSPoint(x: 337, y: 175))
    arrow.lineWidth = 5
    arrow.lineCapStyle = .round
    arrow.lineJoinStyle = .round
    navy.withAlphaComponent(0.65).setStroke()
    arrow.stroke()

    text("After copying, open Lime from Applications.", y: 40,
        font: .systemFont(ofSize: 13), color: navy.withAlphaComponent(0.65))
    NSGraphicsContext.restoreGraphicsState()
    image.addRepresentation(bitmap)
    if scale == 2 {
        try bitmap.representation(using: .png, properties: [:])!
            .write(to: output.appendingPathComponent("background-preview.png"))
    }
}
try image.tiffRepresentation!.write(to: output.appendingPathComponent("background.tiff"))
