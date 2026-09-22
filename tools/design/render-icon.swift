// Renders the app icon source (1024×1024 PNG) on the macOS icon grid.
// Usage: swift tools/design/render-icon.swift <out.png>
// Then: pnpm tauri icon <out.png>
import AppKit

let size: CGFloat = 1024
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png"

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let ctx = NSGraphicsContext.current!.cgContext

// macOS icon grid: 824 pt body centred in 1024, continuous-corner radius ≈ 185.
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
let shape = NSBezierPath(roundedRect: body, xRadius: 185, yRadius: 185)

// Drop shadow per Apple's template.
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28,
              color: NSColor.black.withAlphaComponent(0.28).cgColor)
NSColor(srgbRed: 0.96, green: 0.45, blue: 0.13, alpha: 1).setFill()
shape.fill()
ctx.restoreGState()

// Body: warm orange with a gentle top-to-bottom tonal shift (icon depth, not UI).
ctx.saveGState()
shape.addClip()
let colors = [
    NSColor(srgbRed: 1.00, green: 0.58, blue: 0.20, alpha: 1).cgColor,
    NSColor(srgbRed: 0.93, green: 0.38, blue: 0.08, alpha: 1).cgColor,
] as CFArray
let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: colors, locations: [0, 1])!
ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: 924), end: CGPoint(x: 0, y: 100), options: [])

// Glyph: two nested arches (a portal) and a route line passing through.
let white = NSColor.white
white.setStroke()
func arch(_ w: CGFloat, _ h: CGFloat, _ baseY: CGFloat, _ lw: CGFloat, _ alpha: CGFloat) {
    let p = NSBezierPath()
    let cx: CGFloat = 512
    let left = cx - w / 2, right = cx + w / 2
    p.move(to: CGPoint(x: left, y: baseY))
    p.line(to: CGPoint(x: left, y: baseY + h - w / 2))
    p.appendArc(withCenter: CGPoint(x: cx, y: baseY + h - w / 2), radius: w / 2, startAngle: 180, endAngle: 0, clockwise: true)
    p.line(to: CGPoint(x: right, y: baseY))
    p.lineWidth = lw
    p.lineCapStyle = .round
    p.lineJoinStyle = .round
    white.withAlphaComponent(alpha).setStroke()
    p.stroke()
}
arch(500, 470, 300, 56, 0.55)
arch(300, 330, 300, 56, 1.0)
// Ground line
let ground = NSBezierPath()
ground.move(to: CGPoint(x: 232, y: 300))
ground.line(to: CGPoint(x: 792, y: 300))
ground.lineWidth = 56
ground.lineCapStyle = .round
white.setStroke()
ground.stroke()
ctx.restoreGState()

// Subtle inner stroke for definition on light backgrounds.
NSColor.black.withAlphaComponent(0.08).setStroke()
shape.lineWidth = 2
shape.stroke()

NSGraphicsContext.restoreGraphicsState()
let png = rep.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: out))
print("wrote \(out)")
