import SwiftUI

struct SidebarView: View {
    @Environment(TuckService.self) private var tuckService
    @Binding var selectedEntry: FfiArchiveEntry?
    @State private var searchText = ""

    private var filteredEntries: [FfiArchiveEntry] {
        if searchText.isEmpty {
            return tuckService.entries
        }
        return tuckService.entries.filter {
            $0.originalPath.localizedCaseInsensitiveContains(searchText)
        }
    }

    var body: some View {
        Group {
            if tuckService.selectedDrive == nil {
                ContentUnavailableView(
                    "No Drive Connected",
                    systemImage: "externaldrive.badge.xmark",
                    description: Text("Connect an external drive to get started.")
                )
            } else if tuckService.entries.isEmpty {
                ContentUnavailableView(
                    "No Archived Items",
                    systemImage: "archivebox",
                    description: Text("Drag files here or use the + button to archive.")
                )
            } else {
                List(filteredEntries, id: \.originalPath, selection: $selectedEntry) { entry in
                    EntryRow(entry: entry)
                        .tag(entry)
                }
                .searchable(text: $searchText, prompt: "Filter archives")
            }
        }
        .navigationSplitViewColumnWidth(min: 250, ideal: 300)
    }
}

private struct EntryRow: View {
    let entry: FfiArchiveEntry

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: entry.isDirectory ? "folder.fill" : "doc.fill")
                .foregroundStyle(entry.isDirectory ? .blue : .secondary)
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 2) {
                Text(fileName)
                    .fontWeight(.medium)
                    .lineLimit(1)
                Text(entry.originalPath)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            Text(formatBytes(entry.sizeBytes))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }

    private var fileName: String {
        (entry.originalPath as NSString).lastPathComponent
    }
}

func formatBytes(_ bytes: UInt64) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .file
    return formatter.string(fromByteCount: Int64(bytes))
}
