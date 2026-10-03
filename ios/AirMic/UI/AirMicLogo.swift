import SwiftUI

/// The "Signal" mark: a mic with two arcs, drawn from the 100 × 100 logo artwork.
struct AirMicLogo: View {
    var size: CGFloat = 28

    var body: some View {
        SignalMark()
            .stroke(Theme.accent, style: StrokeStyle(lineWidth: size * 0.08, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

struct SignalMark: Shape {
    func path(in rect: CGRect) -> Path {
        let s = min(rect.width, rect.height) / 100
        // The artwork is drawn with translate(-4, 10).
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: rect.minX + (x - 4) * s, y: rect.minY + (y + 10) * s)
        }
        var path = Path()
        path.addRoundedRect(
            in: CGRect(origin: p(31, 14), size: CGSize(width: 20 * s, height: 36 * s)),
            cornerSize: CGSize(width: 10 * s, height: 10 * s))
        path.move(to: p(26, 44))
        path.addArc(center: p(41, 44), radius: 15 * s, startAngle: .degrees(180), endAngle: .degrees(0), clockwise: true)
        path.move(to: p(41, 59))
        path.addLine(to: p(41, 72))
        path.move(to: p(32, 72))
        path.addLine(to: p(50, 72))
        for radius in [28.0, 42.0] {
            path.move(to: CGPoint(
                x: p(41, 32).x + radius * s * cos(-40 * .pi / 180),
                y: p(41, 32).y + radius * s * sin(-40 * .pi / 180)))
            path.addArc(center: p(41, 32), radius: radius * s, startAngle: .degrees(-40), endAngle: .degrees(40), clockwise: false)
        }
        return path
    }
}

#Preview {
    HStack(spacing: 6) {
        AirMicLogo(size: 64)
        Text("AirMic").font(.system(size: 40, weight: .semibold)).tracking(-1.2)
    }
    .padding()
}
