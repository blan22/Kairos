// kairos 앱 아이콘을 그려 .iconset 폴더로 낸다. build.sh가 부른다.
// 모양: macOS 아이콘 격자(1024 캔버스, 본체 824, 모서리 연속 곡률)를 따른다.
// 배경: 짙은 인디고에서 보라로 가는 그라디언트, 위쪽에 옅은 광택을 얹는다.
// 글리프: 열린 고리와 점 하나. 흘러가는 시간(크로노스) 위의 한 순간(카이로스)이다.
//
//   make_icon <out.iconset> [preview.png]
import AppKit

let canvas: CGFloat = 1024
let bodySize: CGFloat = 824

/// 연속 곡률 모서리(squircle)에 가까운 둥근 사각형이다. 원호 대신 초타원 곡선으로 모서리를 잇는다.
func squircle(in rect: CGRect, radius: CGFloat) -> CGPath {
    let path = CGMutablePath()
    // 모서리 곡선이 변을 따라 원호보다 길게 퍼지도록 1.528배 구간을 쓴다(iOS 연속 곡률 근사).
    let extent = min(radius * 1.528, rect.width / 2)
    let n: CGFloat = 5
    let steps = 48
    let corners: [(CGPoint, CGFloat, CGFloat)] = [
        (CGPoint(x: rect.maxX - extent, y: rect.maxY - extent), 1, 1),
        (CGPoint(x: rect.minX + extent, y: rect.maxY - extent), -1, 1),
        (CGPoint(x: rect.minX + extent, y: rect.minY + extent), -1, -1),
        (CGPoint(x: rect.maxX - extent, y: rect.minY + extent), 1, -1),
    ]
    var first = true
    for (index, corner) in corners.enumerated() {
        let (center, sx, sy) = corner
        for step in 0...steps {
            // 각 모서리에서 사분면을 반시계 방향으로 돈다.
            let t = CGFloat(step) / CGFloat(steps) * (.pi / 2) + CGFloat(index) * (.pi / 2)
            let c = cos(t)
            let s = sin(t)
            let x = center.x + sx * extent * pow(abs(c), 2 / n)
            let y = center.y + sy * extent * pow(abs(s), 2 / n)
            if first {
                path.move(to: CGPoint(x: x, y: y))
                first = false
            } else {
                path.addLine(to: CGPoint(x: x, y: y))
            }
        }
    }
    path.closeSubpath()
    return path
}

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
        green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255,
        alpha: alpha
    )
}

func render(_ px: Int) -> Data {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(
        data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0, space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    ctx.interpolationQuality = .high
    ctx.setShouldAntialias(true)
    let scale = CGFloat(px) / canvas
    ctx.scaleBy(x: scale, y: scale)

    // 작은 크기에서는 그림자와 광택을 줄이고 글리프를 굵게 한다.
    let small = px <= 32
    let tiny = px <= 16
    let inset = (canvas - bodySize) / 2
    // 그림자 자리를 위해 본체를 아주 조금 올린다(애플 격자와 같다).
    let body = CGRect(x: inset, y: inset + (small ? 0 : 6), width: bodySize, height: bodySize)
    let shape = squircle(in: body, radius: 185)

    // 그림자
    if !small {
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28, color: color(0x000000, 0.32))
        ctx.addPath(shape)
        ctx.setFillColor(color(0x2a1f6b))
        ctx.fillPath()
        ctx.restoreGState()
    }

    // 배경 그라디언트: 위 보라, 아래 짙은 인디고
    ctx.saveGState()
    ctx.addPath(shape)
    ctx.clip()
    let base = CGGradient(
        colorsSpace: space,
        colors: [color(0x7d68e0), color(0x4a3cae), color(0x1e1a5c)] as CFArray,
        locations: [0, 0.48, 1]
    )!
    ctx.drawLinearGradient(
        base, start: CGPoint(x: body.minX + body.width * 0.25, y: body.maxY),
        end: CGPoint(x: body.maxX - body.width * 0.25, y: body.minY),
        options: [.drawsBeforeStartLocation, .drawsAfterEndLocation]
    )
    // 위쪽 광택: 위 절반에서만 옅게 사라진다.
    if !tiny {
        let sheen = CGGradient(
            colorsSpace: space,
            colors: [color(0xffffff, 0.18), color(0xffffff, 0.05), color(0xffffff, 0)] as CFArray,
            locations: [0, 0.35, 1]
        )!
        ctx.drawLinearGradient(
            sheen, start: CGPoint(x: body.midX, y: body.maxY),
            end: CGPoint(x: body.midX, y: body.midY), options: [.drawsBeforeStartLocation]
        )
    }
    ctx.restoreGState()

    // 가장자리 안쪽 얇은 빛. 어두운 배경에서도 윤곽이 산다.
    if !small {
        ctx.saveGState()
        ctx.addPath(shape)
        ctx.clip()
        ctx.addPath(shape)
        ctx.setStrokeColor(color(0xffffff, 0.14))
        ctx.setLineWidth(6)
        ctx.strokePath()
        ctx.restoreGState()
    }

    drawGlyph(ctx, body: body, small: small, tiny: tiny)

    let image = ctx.makeImage()!
    let rep = NSBitmapImageRep(cgImage: image)
    rep.size = NSSize(width: px, height: px)
    return rep.representation(using: .png, properties: [:])!
}

/// 열린 고리와 점. 고리는 12시에서 시작해 시계 방향으로 돌다가 1시 반쯤에서 멈추고, 그 틈에 점이 놓인다.
func drawGlyph(_ ctx: CGContext, body: CGRect, small: Bool, tiny: Bool) {
    let center = CGPoint(x: body.midX, y: body.midY)
    let radius = body.width * (tiny ? 0.24 : 0.24)
    let line = body.width * (tiny ? 0.1 : small ? 0.085 : 0.072)
    // 각도는 수학 좌표(반시계, 0 = 3시)다. 점은 1시 반 방향(45도)에 둔다.
    let dotAngle: CGFloat = .pi / 4
    let gap: CGFloat = tiny ? 0.85 : small ? 0.68 : 0.58
    let start = dotAngle - gap
    let end = dotAngle + gap

    ctx.saveGState()
    if !small {
        ctx.setShadow(offset: CGSize(width: 0, height: -6), blur: 16, color: color(0x140c45, 0.35))
    }
    ctx.setStrokeColor(color(0xffffff))
    ctx.setLineWidth(line)
    ctx.setLineCap(.round)
    ctx.addArc(center: center, radius: radius, startAngle: end, endAngle: start + 2 * .pi, clockwise: false)
    ctx.strokePath()

    let dotRadius = line * (tiny ? 1.05 : small ? 0.95 : 0.9)
    let dot = CGPoint(x: center.x + radius * cos(dotAngle), y: center.y + radius * sin(dotAngle))
    ctx.setFillColor(color(0xffffff))
    ctx.fillEllipse(in: CGRect(x: dot.x - dotRadius, y: dot.y - dotRadius, width: dotRadius * 2, height: dotRadius * 2))
    ctx.restoreGState()
}

let arguments = CommandLine.arguments
let out = arguments.count > 1 ? arguments[1] : "AppIcon.iconset"
try FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    try render(size).write(to: URL(fileURLWithPath: "\(out)/icon_\(size)x\(size).png"))
    try render(size * 2).write(to: URL(fileURLWithPath: "\(out)/icon_\(size)x\(size)@2x.png"))
}
print("아이콘을 썼습니다: \(out)")
