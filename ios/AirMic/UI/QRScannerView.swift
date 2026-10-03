import AVFoundation
import SwiftUI

/// Scans the pairing QR code shown by the desktop app (M3.5).
struct QRScannerView: View {
    let onScan: (PairingLink) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var access = AVCaptureDevice.authorizationStatus(for: .video)
    @State private var unrecognized = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            switch access {
            case .authorized:
                CameraPreview { value in
                    if let link = PairingLink(value) {
                        onScan(link)
                    } else {
                        unrecognized = true
                        AccessibilityNotification.Announcement("That isn't an AirMic code").post()
                    }
                }
                .ignoresSafeArea()
                RoundedRectangle(cornerRadius: 28)
                    .strokeBorder(.white.opacity(0.9), lineWidth: 3)
                    .frame(width: 240, height: 240)
                    .accessibilityHidden(true)
            case .notDetermined:
                ProgressView().tint(.white)
            default:
                VStack(spacing: 14) {
                    Text("Camera access is off")
                        .scaledFont(20, weight: .semibold, relativeTo: .title2)
                    Text("Turn on the camera for AirMic in Settings to scan the code, or enter the IP address instead.")
                        .scaledFont(15, relativeTo: .subheadline)
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.white.opacity(0.7))
                    PrimaryButton(title: "Open Settings") {
                        if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                    }
                    .padding(.top, 8)
                }
                .foregroundStyle(.white)
                .padding(32)
            }
            VStack {
                HStack {
                    Spacer()
                    RoundIconButton(systemImage: "xmark", label: "Close") { dismiss() }
                }
                Spacer()
                Text(unrecognized ? "That isn't an AirMic code" : "Point at the QR code in the AirMic desktop app")
                    .scaledFont(15, weight: .medium, relativeTo: .subheadline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 18)
                    .padding(.vertical, 12)
                    .background(.black.opacity(0.55), in: Capsule())
            }
            .padding(.horizontal, Theme.screenPadding)
            .padding(.vertical, 12)
        }
        .task {
            if access == .notDetermined {
                _ = await AVCaptureDevice.requestAccess(for: .video)
                access = AVCaptureDevice.authorizationStatus(for: .video)
            }
        }
    }
}

private struct CameraPreview: UIViewRepresentable {
    let onCode: (String) -> Void

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.previewLayer.session = context.coordinator.session
        view.previewLayer.videoGravity = .resizeAspectFill
        context.coordinator.start()
        return view
    }

    func updateUIView(_ view: PreviewView, context: Context) {
        context.coordinator.onCode = onCode
    }

    static func dismantleUIView(_ view: PreviewView, coordinator: Coordinator) {
        coordinator.stop()
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onCode: onCode)
    }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var previewLayer: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    @MainActor
    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        nonisolated(unsafe) let session = AVCaptureSession()
        var onCode: (String) -> Void
        private var lastValue: String?

        init(onCode: @escaping (String) -> Void) {
            self.onCode = onCode
            super.init()
            guard let camera = AVCaptureDevice.default(for: .video),
                  let input = try? AVCaptureDeviceInput(device: camera),
                  session.canAddInput(input)
            else { return }
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            guard session.canAddOutput(output) else { return }
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
        }

        func start() {
            let session = session
            // startRunning blocks; keep it off the main thread.
            DispatchQueue.global(qos: .userInitiated).async { session.startRunning() }
        }

        func stop() {
            let session = session
            DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
        }

        nonisolated func metadataOutput(
            _ output: AVCaptureMetadataOutput, didOutput metadataObjects: [AVMetadataObject], from connection: AVCaptureConnection
        ) {
            let value = (metadataObjects.first as? AVMetadataMachineReadableCodeObject)?.stringValue
            MainActor.assumeIsolated {
                guard let value, value != lastValue else { return }
                lastValue = value
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                onCode(value)
            }
        }
    }
}
