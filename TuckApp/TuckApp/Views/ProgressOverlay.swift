import SwiftUI

struct ProgressOverlay: View {
    let progress: ProgressReporter
    @State private var displayPhase = ""
    @State private var displayFraction = 0.0
    @State private var displayCompleted: UInt64 = 0
    @State private var displayTotal: UInt64 = 0

    private let timer = Timer.publish(every: 0.1, on: .main, in: .common).autoconnect()

    var body: some View {
        ZStack {
            Color.black.opacity(0.3)
                .ignoresSafeArea()

            VStack(spacing: 16) {
                if !displayPhase.isEmpty {
                    Text(displayPhase)
                        .font(.headline)
                }

                ProgressView(value: displayFraction)
                    .progressViewStyle(.linear)
                    .frame(width: 300)

                if displayTotal > 0 {
                    Text("\(formatBytes(displayCompleted)) / \(formatBytes(displayTotal))")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(32)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        }
        .onReceive(timer) { _ in
            displayPhase = progress.phase
            displayFraction = progress.fractionCompleted
            displayCompleted = progress.completedBytes
            displayTotal = progress.totalBytes
        }
    }
}
