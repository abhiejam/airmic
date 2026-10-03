import SwiftUI
import UIKit

/// Design tokens from the approved mockups: cream light theme, near-black dark theme.
enum Theme {
    static let bg = Color(light: 0xF6F2EA, dark: 0x111114)
    static let surface = Color(light: 0xFFFDF8, dark: 0x1B1B20)
    static let ink = Color(light: 0x1B1A22, dark: 0xF3F0E8)
    /// Increase Contrast: secondary text and lines get stronger.
    static let muted = Color(light: 0x66646E, dark: 0xA3A1AB, highContrastLight: 0x48464F, highContrastDark: 0xC9C7D0)
    static let line = Color(light: 0xE6E0D3, dark: 0x2C2C33, highContrastLight: 0xA9A293, highContrastDark: 0x5A5A66)
    /// Dark accent is the light accent mixed 30% toward white.
    static let accent = Color(light: 0x5146E5, dark: 0x857EED)
    static let onAccent = Color(light: 0xFFFFFF, dark: 0x111114)
    static let accentSoft = Color(light: 0x5146E5, dark: 0x857EED, lightAlpha: 0.08, darkAlpha: 0.12)
    static let accentShadow = Color(light: 0x5146E5, dark: 0x857EED, lightAlpha: 0.5, darkAlpha: 0.45)
    static let accentBar = Color(light: 0x5146E5, dark: 0x857EED, lightAlpha: 0.2, darkAlpha: 0.28)
    static let warn = Color(light: 0xC2410C, dark: 0xF0894A)
    static let onWarn = Color(light: 0xFFFFFF, dark: 0x111114)
    static let ok = Color(light: 0x2F8F5B, dark: 0x4CC38A)

    static let screenPadding: CGFloat = 24
    static let buttonHeight: CGFloat = 56
    static let primaryWidth: CGFloat = 280
    static let iconButton: CGFloat = 44
    static let cardRadius: CGFloat = 22
}

extension Color {
    init(
        light: UInt32, dark: UInt32, lightAlpha: CGFloat = 1, darkAlpha: CGFloat = 1,
        highContrastLight: UInt32? = nil, highContrastDark: UInt32? = nil
    ) {
        self.init(uiColor: UIColor { traits in
            let high = traits.accessibilityContrast == .high
            return traits.userInterfaceStyle == .dark
                ? UIColor(hex: high ? highContrastDark ?? dark : dark, alpha: darkAlpha)
                : UIColor(hex: high ? highContrastLight ?? light : light, alpha: lightAlpha)
        })
    }
}

extension View {
    /// SF Pro at the mockup's point size, scaled with Dynamic Type like `style`.
    func scaledFont(
        _ size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default,
        relativeTo style: Font.TextStyle = .body
    ) -> some View {
        modifier(ScaledFont(size: size, weight: weight, design: design, style: style))
    }
}

private struct ScaledFont: ViewModifier {
    @ScaledMetric private var size: CGFloat
    private let weight: Font.Weight
    private let design: Font.Design

    init(size: CGFloat, weight: Font.Weight, design: Font.Design, style: Font.TextStyle) {
        _size = ScaledMetric(wrappedValue: size, relativeTo: style)
        self.weight = weight
        self.design = design
    }

    func body(content: Content) -> some View {
        content.font(.system(size: size, weight: weight, design: design))
    }
}

extension UIColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha)
    }
}

extension View {
    /// Small uppercase section label ("NEARBY", "FOCUS SESSION").
    func sectionLabelStyle() -> some View {
        scaledFont(12, relativeTo: .caption).tracking(1.2).textCase(.uppercase).foregroundStyle(Theme.muted)
    }
}

/// 44 pt round icon button with a hairline border (settings, back, close).
struct RoundIconButton: View {
    let systemImage: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 17, weight: .medium))
                .frame(width: Theme.iconButton, height: Theme.iconButton)
                .background(Theme.surface, in: Circle())
                .overlay(Circle().strokeBorder(Theme.line))
        }
        .buttonStyle(.plain)
        .foregroundStyle(Theme.ink)
        .accessibilityLabel(label)
    }
}

/// 280 × 56 accent capsule.
struct PrimaryButton: View {
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .scaledFont(16, weight: .medium, relativeTo: .body)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .padding(.horizontal, 16)
                .frame(width: Theme.primaryWidth)
                .frame(minHeight: Theme.buttonHeight)
                .background(Theme.accent, in: Capsule())
                .foregroundStyle(Theme.onAccent)
        }
        .buttonStyle(.plain)
    }
}
