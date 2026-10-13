// Headless CoreText/CoreGraphics ink oracle; no AppKit, window, or pasteboard.
// Run from the repo root:
// swiftc -O gpui/tests/typography-ink.swift -o /tmp/tau-typography-ink && /tmp/tau-typography-ink
import Foundation
import CoreText
import CoreGraphics

enum Mask { case none, horizontal, oldTruncate, viewport }
let weights = [400, 500, 600, 650, 700]
let sizes: [CGFloat] = [10, 11, 12, 13]
let scales: [CGFloat] = [1, 1.25, 1.5, 2]
let phases: [CGFloat] = [0, 0.25, 0.5, 0.75]
let samples = ["gpui-rewrite", "High", "gypq", "HH"]
let themes = [("light", [252, 252, 252], [92, 97, 102]),
              ("dark", [16, 20, 28], [191, 189, 182])].map { theme, bg, fg in
    (theme, (0...255).map { alpha in
        zip(bg, fg).reduce(0) { value, channel in
            let (b, f) = channel
            return (value << 8) | Int((Double(b) + Double(f - b) * Double(alpha) / 255).rounded())
        }
    })
}
var cases = 0, lostPixels = 0, affectedLabels = Set<String>()
var results: [[String: Any]] = []

func raster(_ text: String, _ font: CTFont, _ size: CGFloat, _ scale: CGFloat,
            _ phase: CGFloat, _ mask: Mask) -> [UInt8] {
    let width = Int(180 * scale), height = Int(64 * scale)
    var pixels = [UInt8](repeating: 0, count: width * height)
    pixels.withUnsafeMutableBytes { storage in
        let context = CGContext(data: storage.baseAddress, width: width, height: height,
                                bitsPerComponent: 8, bytesPerRow: width,
                                space: CGColorSpaceCreateDeviceGray(), bitmapInfo: 0)!
        context.scaleBy(x: scale, y: scale)
        context.setShouldAntialias(true)
        context.setShouldSmoothFonts(false) // Tau's grayscale-font policy
        context.setFillColor(gray: 1, alpha: 1)
        let top: CGFloat = 24 + phase
        // GPUI centers ascent + descent in the authored (device-snapped) line box.
        let leading = ceil(size * scale - 0.5) / scale // GPUI: half toward zero
        let baseline = top + (leading - CTFontGetAscent(font) - CTFontGetDescent(font)) / 2
                       + CTFontGetAscent(font)
        switch mask {
        case .none: break
        case .horizontal:
            context.clip(to: CGRect(x: 10, y: 0, width: 145, height: 64))
        case .oldTruncate:
            context.clip(to: CGRect(x: 10, y: 64 - top - leading, width: 145, height: leading))
        case .viewport:
            // An intentional enclosing viewport must still constrain visible-axis ink.
            context.clip(to: CGRect(x: 10, y: 64 - top - leading, width: 145, height: leading))
        }
        let line = CTLineCreateWithAttributedString(NSAttributedString(
            string: text, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font,
                                      NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true]))
        context.textPosition = CGPoint(x: 14, y: 64 - baseline)
        CTLineDraw(line, context)
    }
    return pixels
}

for weight in weights {
    let url = URL(fileURLWithPath: "gpui/assets/product-fonts/TauInter-\(weight).ttf")
    let descriptors = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as! [CTFontDescriptor]
    for size in sizes {
        let font = CTFontCreateWithFontDescriptor(descriptors[0], size, nil)
        for scale in scales { for phase in phases { for text in samples {
            let full = raster(text, font, size, scale, phase, .none)
            let fixed = raster(text, font, size, scale, phase, .horizontal)
            let old = raster(text, font, size, scale, phase, .oldTruncate)
            let viewport = raster(text, font, size, scale, phase, .viewport)
            // All these short labels fit horizontally, so every painted ink pixel must survive.
            precondition(full.contains(where: { $0 > 0 }), "empty glyph raster")
            precondition(fixed == full, "horizontal truncation changed vertical ink")
            precondition(viewport == old, "explicit viewport must retain two-axis clipping")
            let lost = zip(full, old).filter { $0 > $1 }.count
            if text == "HH" { precondition(lost == 0, "non-descender negative control clipped") }
            if lost > 0 { affectedLabels.insert(text) }
            // Composite the actual coverage samples against Tau's authored theme colors.
            for (theme, colors) in themes {
                let lostInTheme = zip(full, old).filter { colors[Int($0)] != colors[Int($1)] }.count
                precondition(zip(full, fixed).allSatisfy { colors[Int($0)] == colors[Int($1)] })
                if text == "HH" { precondition(lostInTheme == 0) }
                cases += 1
                lostPixels += lostInTheme
                results.append(["theme": theme, "weight": weight, "size": Double(size),
                                "scale": Double(scale), "phase": Double(phase), "text": text,
                                "lostInkPixelsBefore": lostInTheme, "lostInkPixelsAfter": 0])
            }
        }}}
    }
}
precondition(affectedLabels.isSuperset(of: ["gpui-rewrite", "High", "gypq"]), "clipping negative controls did not reproduce")
// A long string confirms that the fix still masks horizontal overflow.
let descriptor = (CTFontManagerCreateFontDescriptorsFromURL(URL(fileURLWithPath:
    "gpui/assets/product-fonts/TauInter-400.ttf") as CFURL) as! [CTFontDescriptor])[0]
let font = CTFontCreateWithFontDescriptor(descriptor, 13, nil)
let long = String(repeating: "gypq debugging ", count: 12)
let full = raster(long, font, 13, 2, 0.25, .none)
let horizontal = raster(long, font, 13, 2, 0.25, .horizontal)
precondition(full != horizontal, "long-label horizontal negative control did not overflow")
precondition(horizontal.contains(where: { $0 > 0 }), "horizontal mask erased label")
let metrics = sizes.map { size -> [String: Any] in
    let face = CTFontCreateWithFontDescriptor(descriptor, size, nil)
    return ["size": Double(size), "ascent": Double(CTFontGetAscent(face)),
            "descent": Double(CTFontGetDescent(face))]
}
let report: [String: Any] = ["backend": "CoreText/CoreGraphics headless; not GPUI Root readback",
    "regularFaceMetrics": metrics, "cases": cases, "lostInkPixelsBefore": lostPixels, "lostInkPixelsAfter": 0,
    "affectedLabels": affectedLabels.sorted(), "results": results]
let json = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
FileHandle.standardOutput.write(json)
FileHandle.standardOutput.write(Data("\n".utf8))
