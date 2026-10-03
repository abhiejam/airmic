import Network
import SwiftUI

struct ConnectView: View {
    @Environment(StreamSession.self) private var session
    @Environment(\.dismiss) private var dismiss
    @State private var showManualEntry = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            RoundIconButton(systemImage: "chevron.left", label: "Back") { dismiss() }

            Text("Connect to a computer")
                .font(.system(size: 28, weight: .semibold))
                .tracking(-0.6)
                .padding(.top, 28)
            Text("Open AirMic on your computer. Both devices need to be on the same Wi-Fi.")
                .font(.system(size: 15))
                .lineSpacing(3)
                .foregroundStyle(Theme.muted)
                .padding(.top, 10)

            Text("Nearby")
                .sectionLabelStyle()
                .padding(.top, 40)

            Group {
                if let recent = session.recentComputer {
                    ComputerRow(computer: recent) {
                        Task {
                            await session.connect(to: recent)
                            dismiss()
                        }
                    }
                } else {
                    Text("Computers running the AirMic desktop app will show up here.")
                        .font(.system(size: 14))
                        .foregroundStyle(Theme.muted)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(20)
                        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
                        .overlay(RoundedRectangle(cornerRadius: Theme.cardRadius).strokeBorder(Theme.line))
                }
            }
            .padding(.top, 14)

            Text("Computer not listed? Get the AirMic desktop app.")
                .font(.system(size: 13))
                .foregroundStyle(Theme.muted)
                .frame(maxWidth: .infinity)
                .padding(.top, 16)

            Spacer()

            Text("Other ways").sectionLabelStyle()
            HStack(spacing: 12) {
                OtherWayButton(title: "Scan QR code", systemImage: "qrcode.viewfinder") {}
                    .disabled(true)
                    .opacity(0.45)
                    .accessibilityHint("Available once the desktop app shows a pairing code")
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
        .sheet(isPresented: $showManualEntry) {
            ManualEntrySheet(initial: session.recentComputer) { computer in
                showManualEntry = false
                Task {
                    await session.connect(to: computer)
                    dismiss()
                }
            }
            .presentationDetents([.medium])
        }
    }

    private struct ComputerRow: View {
        let computer: Computer
        let action: () -> Void

        var body: some View {
            Button(action: action) {
                HStack(spacing: 12) {
                    Image(systemName: "desktopcomputer")
                        .font(.system(size: 19))
                        .foregroundStyle(Theme.accent)
                        .frame(width: 44, height: 44)
                        .background(Theme.accentSoft, in: RoundedRectangle(cornerRadius: 14))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(computer.name)
                            .font(.system(size: 16, weight: .semibold))
                        Text("Recent · \(computer.host)")
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(Theme.muted)
                    }
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(Theme.muted)
                }
                .padding(20)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
                .overlay(RoundedRectangle(cornerRadius: Theme.cardRadius).strokeBorder(Theme.accent, lineWidth: 1.5))
            }
            .buttonStyle(.plain)
        }
    }

    private struct OtherWayButton: View {
        let title: String
        let systemImage: String
        let action: () -> Void

        var body: some View {
            Button(action: action) {
                Label(title, systemImage: systemImage)
                    .font(.system(size: 15, weight: .medium))
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

/// Manual IP entry (M3.6).
struct ManualEntrySheet: View {
    let onConnect: (Computer) -> Void
    @State private var host: String
    @State private var port: String
    @State private var name: String
    @FocusState private var hostFocused: Bool

    init(initial: Computer?, onConnect: @escaping (Computer) -> Void) {
        self.onConnect = onConnect
        _host = State(initialValue: initial?.host ?? "")
        _port = State(initialValue: String(initial?.port ?? 5555))
        _name = State(initialValue: initial?.name ?? "")
    }

    static func isValidHost(_ host: String) -> Bool {
        if IPv4Address(host) != nil { return true }
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: ".-"))
        return !host.isEmpty && host.count <= 253
            && host.rangeOfCharacter(from: allowed.inverted) == nil
            && host.first != "." && host.first != "-"
            && host.contains(where: \.isLetter) // otherwise it's a malformed IP
    }

    private var trimmedHost: String { host.trimmingCharacters(in: .whitespaces) }
    private var portNumber: UInt16? { UInt16(port).flatMap { $0 > 0 ? $0 : nil } }
    private var isValid: Bool { Self.isValidHost(trimmedHost) && portNumber != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Enter IP address")
                .font(.system(size: 22, weight: .semibold))
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
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.warn)
            }
            Spacer(minLength: 0)
            PrimaryButton(title: "Connect") {
                guard isValid, let portNumber else { return }
                let name = name.trimmingCharacters(in: .whitespaces)
                onConnect(Computer(name: name.isEmpty ? trimmedHost : name, host: trimmedHost, port: portNumber))
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
            .font(.system(size: 17))
            .padding(.horizontal, 16)
            .frame(height: 52)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Theme.line))
    }
}

#Preview {
    NavigationStack { ConnectView() }
        .environment(StreamSession())
}
