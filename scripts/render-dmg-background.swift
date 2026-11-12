// Run from the repository root: swift scripts/render-dmg-background.swift
// Finder displays this 2x PNG at 660 x 392 points below its 28-point title bar.
import AppKit

let width = 660
let height = 392
guard let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: width * 2, pixelsHigh: height * 2,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
    fatalError("Could not create the DMG background canvas")
}
bitmap.size = NSSize(width: width, height: height)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = context
context.cgContext.scaleBy(x: 2, y: 2)
NSColor.white.setFill()
NSRect(x: 0, y: 0, width: width, height: height).fill()

func point(_ x: CGFloat, _ y: CGFloat) -> NSPoint {
    NSPoint(x: x, y: CGFloat(height) - y)
}

let arrow = NSBezierPath()
arrow.move(to: point(274, 158))
arrow.curve(to: point(386, 158), controlPoint1: point(310, 135), controlPoint2: point(350, 135))
arrow.move(to: point(374, 140))
arrow.line(to: point(386, 158))
arrow.line(to: point(365, 158))
arrow.lineWidth = 2.8
arrow.lineCapStyle = .round
arrow.lineJoinStyle = .round
NSColor(calibratedWhite: 0.08, alpha: 1).setStroke()
arrow.stroke()

let accents = NSBezierPath()
for (start, end) in [(point(551, 87), point(553, 70)),
                     (point(563, 94), point(580, 79)),
                     (point(569, 104), point(586, 103))] {
    accents.move(to: start)
    accents.line(to: end)
}
accents.lineWidth = 2.8
NSColor(calibratedRed: 0.12, green: 0.43, blue: 1, alpha: 1).setStroke()
accents.stroke()

let text = "Drag \"Vary AI\" to the Applications folder" as NSString
let attributes: [NSAttributedString.Key: Any] = [
    .font: NSFont.systemFont(ofSize: 21, weight: .light),
    .foregroundColor: NSColor(calibratedWhite: 0.12, alpha: 1),
]
let size = text.size(withAttributes: attributes)
text.draw(at: NSPoint(x: (CGFloat(width) - size.width) / 2, y: CGFloat(height) - 307),
          withAttributes: attributes)
NSGraphicsContext.restoreGraphicsState()
guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fatalError("Could not encode the DMG background")
}
try png.write(to: URL(fileURLWithPath: "src-tauri/icons/dmg-background.png"))
