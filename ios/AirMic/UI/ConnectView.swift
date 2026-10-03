import AVFoundation
import SwiftUI

struct ConnectView: View {
    @Environment(StreamSession.self) private var session
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var discovery = Discovery()
    @AppStorage("localNetwork.explained") private var localNetworkExplained = false
    @State private var showManualEntry = false
    @State private var showScanner = false
    /// The computer the user tapped; its card shows progress, the code entry or an error.
    @State private var selectedID: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            RoundIconButton(systemImage: "chevron.left", label: "Back") {
                session.cancelPairing()
                dismiss()
            }

            Text("Connect to a computer")
                .scaledFont(28, weight: .semibold, relativeTo: .title)
                .tracking(-0.6)
                .padding(.top, 28)
                .accessibilityAddTraits(.isHeader)
            Text("Open AirMic on your computer. Both devices need to be on the same Wi-Fi.")
                .scaledFont(15, relativeTo: .subheadline)
                .lineSpacing(3)
                .foregroundStyle(Theme.muted)
                .padding(.top, 10)

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    nearbyHeader
                    nearbyContent
                    Text("Computer not listed? Get the AirMic desktop app.")
                        .scaledFont(13, relativeTo: .footnote)
                        .foregroundStyle(Theme.muted)
                        .frame(maxWidth: .infinity)
                        .padding(.top, 2)
                }
                .padding(.top, 40)
                .padding(.bottom, 16)
            }
            .scrollIndicators(.hidden)
            .scrollBounceBehavior(.basedOnSize)

            Text("Other ways").sectionLabelStyle()
            HStack(spacing: 12) {
                OtherWayButton(title: "Scan QR code", systemImage: "qrcode.viewfinder") { showScanner = true }
                OtherWayButton(title: "Enter IP address", systemImage: "number") { showManualEntry = true }
            }
            .padding(.top, 14)
        }
        .padding(.horizontal, Theme.screenPadding)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Theme.bg.ignoresSafeArea())
        .foregroundStyle(Theme.ink)
        .toolbar(.hidden, for: .navigationBar)
        .onAppear {
            if localNetworkExplained { discovery.start() }
            if session.isPairing { selectedID = session.computer?.id }
        }
        .onDisappear { discovery.stop() }
        .onChange(of: session.phase) { _, phase in
            if phase == .live || phase == .paused { dismiss() }
        }
        .sheet(isPresented: $showManualEntry) {
            ManualEntrySheet(initial: session.knownComputers.first { $0.serviceName == nil }) { computer in
                showManualEntry = false
                connect(computer)
            }
            .presentationDetents([.medium])
        }
        .fullScreenCover(isPresented: $showScanner) {
            QRScannerView { link in
                showScanner = false
                connect(link.computer, pairingCode: link.code)
            }
        }
    }

    // MARK: - Nearby

    private var nearbyHeader: some View {
        HStack {
            Text("Nearby").sectionLabelStyle()
            Spacer()
            if discovery.state == .searching {
                HStack(spacing: 6) {
                    Circle().fill(Theme.accent).frame(width: 6, height: 6)
                    Text("Searching")
                }
                .scaledFont(13, relativeTo: .footnote)
                .foregroundStyle(Theme.muted)
            }
        }
    }

    @ViewBuilder
    private var nearbyContent: some View {
        if !localNetworkExplained {
            NoticeCard(
                title: "Find your computer",
                message: "AirMic looks for computers running AirMic on your Wi-Fi. iOS will ask you to allow this.",
                button: "Continue"
            ) {
                localNetworkExplained = true
                discovery.start()
            }
        } else if discovery.state == .denied {
            NoticeCard(
                title: "Local Network is off",
                message: "AirMic can't see computers on your Wi-Fi. Turn on Local Network for AirMic in Settings, or enter the IP address below.",
                button: "Open Settings"
            ) {
                if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
            }
        } else if rows.isEmpty {
            Text("Computers running the AirMic desktop app will show up here.")
                .scaledFont(14, relativeTo: .subheadline)
                .foregroundStyle(Theme.muted)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(20)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
                .overlay(RoundedRectangle(cornerRadius: Theme.cardRadius).strokeBorder(Theme.line))
        }
        ForEach(rows) { row in
            ComputerCard(
                computer: row.computer,
                subtitle: row.subtitle,
                isSelected: row.computer.id == selectedID,
                status: row.computer.id == selectedID ? cardStatus : .idle,
                onTap: { connect(row.computer) },
                onCode: { session.submitPairingCode($0) })
        }
    }

    private struct Row: Identifiable {
        let computer: Computer
        let subtitle: String
        var id: String { computer.id }
    }

    /// Discovered computers first, then known ones that aren't advertising right now.
    private var rows: [Row] {
        let known = Dictionary(session.knownComputers.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var rows = discovery.computers.map { found in
            var computer = found
            computer.host = known[found.id]?.host
            return Row(computer: computer, subtitle: computer.host.map { "On this Wi-Fi · \($0)" } ?? "On this Wi-Fi")
        }
        let shown = Set(rows.map(\.id))
        for computer in session.knownComputers where !shown.contains(computer.id) {
            rows.append(Row(computer: computer, subtitle: "Recent · \(computer.host ?? computer.serviceName ?? "")"))
        }
        return rows
    }

    private var cardStatus: ComputerCard.Status {
        switch session.phase {
        case .connecting: .connecting
        case .pairing(let step): .pairing(step)
        case .failed(let message): .failed(message)
        default: .idle
        }
    }

    private func connect(_ computer: Computer, pairingCode: String? = nil) {
        selectedID = computer.id
        Task { await session.connect(to: computer, pairingCode: pairingCode) }
    }

    // MARK: - Pieces

    private struct NoticeCard: View {
        let title: String
        let message: String
        let button: String
        let action: () -> Void

        var body: some View {
            VStack(alignment: .leading, spacing: 10) {
                Text(title).scaledFont(16, weight: .semibold, relativeTo: .body)
                Text(message)
                    .scaledFont(14, relativeTo: .subheadline)
                    .lineSpacing(2)
                    .foregroundStyle(Theme.muted)
                Button(action: action) {
                    Text(button)
                        .scaledFont(15, weight: .medium, relativeTo: .subheadline)
                        .padding(.horizontal, 20)
                        .frame(height: 44)
                        .background(Theme.accent, in: Capsule())
                        .foregroundStyle(Theme.onAccent)
                }
                .buttonStyle(.plain)
                .padding(.top, 4)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
            .overlay(RoundedRectangle(cornerRadius: Theme.cardRadius).strokeBorder(Theme.line))
        }
    }

    private struct OtherWayButton: View {
        let title: String
        let systemImage: String
        let action: () -> Void

        var body: some View {
            Button(action: action) {
                Label(title, systemImage: systemImage)
                    .scaledFont(15, weight: .medium, relativeTo: .subheadline)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                    .frame(maxWidth: .infinity)
                    .frame(height: Theme.buttonHeight)
                    .background(Theme.surface, in: Capsule())
                    .overlay(Capsule().strokeBorder(Theme.line))
            }
            .buttonStyle(.plain)
        }
    }
}

/// One computer. When selected it expands to show connecting, the code entry, or an error.
private struct ComputerCard: View {
    enum Status: Equatable {
        case idle
        case connecting
        case pairing(StreamSession.PairingStep)
        case failed(String)
    }

    let computer: Computer
    let subtitle: String
    let isSelected: Bool
    let status: Status
    let onTap: () -> Void
    let onCode: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Button(action: onTap) {
                HStack(spacing: 12) {
                    Image(systemName: "desktopcomputer")
                        .font(.system(size: 19))
                        .foregroundStyle(isSelected ? Theme.accent : Theme.muted)
                        .frame(width: 44, height: 44)
                        .background(isSelected ? Theme.accentSoft : Theme.bg, in: RoundedRectangle(cornerRadius: 14))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(computer.name)
                            .scaledFont(16, weight: .semibold, relativeTo: .body)
                            .lineLimit(1)
                        Text(subtitle)
                            .scaledFont(12, design: .monospaced, relativeTo: .footnote)
                            .foregroundStyle(Theme.muted)
                            .lineLimit(1)
                    }
                    Spacer()
                    trailing
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(status == .connecting || isPairing)
            .accessibilityValue(accessibilityStatus)
            .accessibilityHint(status == .idle || isFailed ? "Connects to this computer" : "")

            if isPairing || isFailed {
                Rectangle().fill(Theme.line).frame(height: 1)
                details
            }
        }
        .padding(20)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.cardRadius)
                .strokeBorder(isSelected ? Theme.accent : Theme.line, lineWidth: isSelected ? 1.5 : 1))
        .animation(.snappy(duration: 0.25), value: status)
    }

    private var isPairing: Bool {
        if case .pairing = status { return true }
        return false
    }

    private var isFailed: Bool {
        if case .failed = status { return true }
        return false
    }

    private var accessibilityStatus: String {
        switch status {
        case .idle: ""
        case .connecting, .pairing(.checking): "Connecting"
        case .pairing: "Needs the pairing code"
        case .failed(let message): message
        }
    }

    @ViewBuilder
    private var trailing: some View {
        switch status {
        case .connecting, .pairing(.checking):
            ProgressView()
        case .pairing:
            Image(systemName: "lock")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(Theme.accent)
        default:
            Image(systemName: "chevron.right")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(Theme.muted)
        }
    }

    @ViewBuilder
    private var details: some View {
        switch status {
        case .pairing(let step):
            VStack(alignment: .leading, spacing: 14) {
                Text(step == .wrongCode
                     ? "That code didn't match. Check the code on \(computer.name)."
                     : "Enter the code shown on \(computer.name)")
                    .scaledFont(14, relativeTo: .subheadline)
                    .foregroundStyle(step == .wrongCode ? Theme.warn : Theme.muted)
                PairingCodeField(isChecking: step == .checking, resetTrigger: step == .wrongCode, onComplete: onCode)
            }
        case .failed(let message):
            Text(message)
                .scaledFont(14, relativeTo: .subheadline)
                .foregroundStyle(Theme.warn)
        default:
            EmptyView()
        }
    }
}

/// Four digit boxes over one hidden number field.
private struct PairingCodeField: View {
    let isChecking: Bool
    let resetTrigger: Bool
    let onComplete: (String) -> Void
    @State private var code = ""
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            TextField("", text: $code)
                .keyboardType(.numberPad)
                .textContentType(.oneTimeCode)
                .focused($focused)
                .opacity(0.02)
                .accessibilityLabel("Pairing code")
            HStack(spacing: 10) {
                ForEach(0..<4, id: \.self) { index in
                    let digit = index < code.count ? String(Array(code)[index]) : ""
                    Text(digit)
                        .font(.system(size: 26, weight: .medium))
                        .frame(maxWidth: .infinity)
                        .frame(height: 60)
                        .background(Theme.bg, in: RoundedRectangle(cornerRadius: 14))
                        .overlay(
                            RoundedRectangle(cornerRadius: 14)
                                .strokeBorder(
                                    focused && index == min(code.count, 3) ? Theme.accent : Theme.line,
                                    lineWidth: focused && index == min(code.count, 3) ? 2 : 1))
                }
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
        }
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
        .disabled(isChecking)
        .opacity(isChecking ? 0.5 : 1)
        .onAppear { focused = true }
        .onChange(of: code) { _, newValue in
            let digits = String(newValue.filter(\.isNumber).prefix(4))
            if digits != newValue { code = digits }
            if digits.count == 4 { onComplete(digits) }
        }
        .onChange(of: resetTrigger) { _, wrong in
            if wrong {
                code = ""
                focused = true
                AccessibilityNotification.Announcement("That code didn't match. Try again.").post()
            }
        }
    }
}

/// Manual IP entry (M3.6).
struct ManualEntrySheet: View {
    let onConnect: (Computer) -> Void
    @State private var host: String
    @State private var port: String
    @State private var name: String
    @FocusState private var hostFocused: Bool

    init(initial: Computer?, onConnect: @escaping (Computer) -> Void) {
        self.onConnect = onConnect
        _host = State(initialValue: initial?.serviceName == nil ? initial?.host ?? "" : "")
        _port = State(initialValue: String(initial?.port ?? AirMicProtocol.controlPort))
        _name = State(initialValue: initial?.name ?? "")
    }

    nonisolated static func isValidHost(_ host: String) -> Bool {
        // Only digits and dots: a full dotted quad. (IPv4Address accepts "192.168.20" shorthand.)
        if host.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == ".") }) {
            let parts = host.split(separator: ".", omittingEmptySubsequences: false)
            return parts.count == 4 && parts.allSatisfy { !$0.isEmpty && $0.count <= 3 && UInt8($0) != nil }
        }
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: ".-"))
        return !host.isEmpty && host.count <= 253
            && host.rangeOfCharacter(from: allowed.inverted) == nil
            && host.first != "." && host.first != "-"
    }

    private var trimmedHost: String { host.trimmingCharacters(in: .whitespaces) }
    private var portNumber: UInt16? { UInt16(port).flatMap { $0 > 0 ? $0 : nil } }
    private var isValid: Bool { Self.isValidHost(trimmedHost) && portNumber != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Enter IP address")
                .scaledFont(22, weight: .semibold, relativeTo: .title2)
                .accessibilityAddTraits(.isHeader)
            VStack(spacing: 10) {
                field("IP address, e.g. 192.168.1.42", text: $host, keyboard: .numbersAndPunctuation)
                    .focused($hostFocused)
                HStack(spacing: 10) {
                    field("Port", text: $port, keyboard: .numberPad)
                        .frame(width: 110)
                    field("Name (optional)", text: $name, keyboard: .default)
                }
            }
            if !trimmedHost.isEmpty && !Self.isValidHost(trimmedHost) {
                Text("That doesn't look like an IP address.")
                    .scaledFont(13, relativeTo: .footnote)
                    .foregroundStyle(Theme.warn)
            }
            Spacer(minLength: 0)
            PrimaryButton(title: "Connect") {
                guard isValid, let portNumber else { return }
                let name = name.trimmingCharacters(in: .whitespaces)
                onConnect(.manual(name: name.isEmpty ? trimmedHost : name, host: trimmedHost, port: portNumber))
            }
            .disabled(!isValid)
            .opacity(isValid ? 1 : 0.5)
            .frame(maxWidth: .infinity)
        }
        .padding(Theme.screenPadding)
        .background(Theme.bg.ignoresSafeArea())
        .foregroundStyle(Theme.ink)
        .onAppear { hostFocused = host.isEmpty }
    }

    private func field(_ placeholder: String, text: Binding<String>, keyboard: UIKeyboardType) -> some View {
        TextField(placeholder, text: text)
            .keyboardType(keyboard)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .scaledFont(17, relativeTo: .body)
            .padding(.horizontal, 16)
            .frame(minHeight: 52)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Theme.line))
    }
}

