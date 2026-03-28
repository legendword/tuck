import SwiftUI

struct DetailView: View {
    let entry: FfiArchiveEntry
    @Binding var showingRestoreConfirm: Bool
    @Binding var keepArchiveOnRestore: Bool

    var body: some View {
        VStack(spacing: 0) {
            // Header
            VStack(spacing: 12) {
                Image(systemName: entry.isDirectory ? "folder.fill" : "doc.fill")
                    .font(.system(size: 48))
                    .foregroundStyle(entry.isDirectory ? .blue : .secondary)

                Text(fileName)
                    .font(.title2)
                    .fontWeight(.semibold)

                Text(entry.originalPath)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            .padding(.top, 32)
            .padding(.bottom, 24)

            // Info grid
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 8) {
                GridRow {
                    Text("Size")
                        .foregroundStyle(.secondary)
                    Text(formatBytes(entry.sizeBytes))
                }
                GridRow {
                    Text("Type")
                        .foregroundStyle(.secondary)
                    Text(entry.isDirectory ? "Directory" : "File")
                }
                GridRow {
                    Text("Archived")
                        .foregroundStyle(.secondary)
                    Text(formatDate(entry.archivedAt))
                }
                GridRow {
                    Text("Drive")
                        .foregroundStyle(.secondary)
                    Text(entry.driveName)
                }
                GridRow {
                    Text("Files")
                        .foregroundStyle(.secondary)
                    Text("\(entry.checksums.count)")
                }
            }
            .padding(.horizontal, 32)

            Spacer()

            // Action button
            Button {
                showingRestoreConfirm = true
            } label: {
                Label("Restore", systemImage: "arrow.down.doc")
                    .frame(maxWidth: 200)
            }
            .controlSize(.large)
            .buttonStyle(.borderedProminent)
            .padding(.bottom, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var fileName: String {
        (entry.originalPath as NSString).lastPathComponent
    }

    private func formatDate(_ timestamp: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
}
