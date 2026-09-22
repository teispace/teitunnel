// Renders the menu bar (tray) template icon: black glyph on transparent, 18 pt @2x.
// macOS tints template images for light/dark menu bars automatically.
// Usage: swift tools/design/render-tray-icon.swift <out.png>
import AppKit

let px: CGFloat = 36
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "tray.png"
let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(px), pixelsHigh: Int(px),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSColor.black.setStroke()

// Same glyph as the app icon: two nested arches over a ground line.
func arch(_ w: CGFloat, _ top: CGFloat, _ base: CGFloat, _ lw: CGFloat, _ alpha: CGFloat) {
    let p = NSBezierPath()
    let cx = px / 2, r = w / 2
    p.move(to: CGPoint(x: cx - r, y: base))
    p.line(to: CGPoint(x: cx - r, y: top - r))
    p.appendArc(withCenter: CGPoint(x: cx, y: top - r), radius: r, startAngle: 180, endAngle: 0, clockwise: true)
    p.line(to: CGPoint(x: cx + r, y: base))
    p.lineWidth = lw
    p.lineCapStyle = .round
    NSColor.black.withAlphaComponent(alpha).setStroke()
    p.stroke()
}
arch(26, 31, 8, 3.2, 0.55)
arch(14, 23, 8, 3.2, 1)
let ground = NSBezierPath()
ground.move(to: CGPoint(x: 3.5, y: 7))
ground.line(to: CGPoint(x: px - 3.5, y: 7))
ground.lineWidth = 3.2
ground.lineCapStyle = .round
NSColor.black.setStroke()
ground.stroke()

NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
print("wrote \(out)")
