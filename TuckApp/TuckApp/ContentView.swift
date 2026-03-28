import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Environment(TuckService.self) private var tuckService

    @State private var selectedEntry: FfiArchiveEntry?
    @State private var showingAddPicker = false
    @State private var showingRestoreConfirm = false
    @State private var keepArchiveOnRestore = false

    var body: some View {
        NavigationSplitView {
            SidebarView(selectedEntry: $selectedEntry)
        } detail: {
            if let entry = selectedEntry {
                DetailView(
                    entry: entry,
                    showingRestoreConfirm: $showingRestoreConfirm,
                    keepArchiveOnRestore: $keepArchiveOnRestore
                )
            } else {
                ContentUnavailableView(
                    "No Selection",
                    systemImage: "archivebox",
                    description: Text("Select an archived item to view its details.")
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .automatic) {
                if tuckService.drives.count > 1 {
                    DrivePicker()
                } else if let drive = tuckService.selectedDrive {
                    HStack(spacing: 4) {
                        Image(systemName: "externaldrive.fill")
                        Text(drive.name)
                    }
                    .foregroundStyle(.secondary)
                }
            }
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    showingAddPicker = true
                } label: {
                    Label("Add", systemImage: "plus")
                }
                .disabled(tuckService.selectedDrive == nil || tuckService.isLoading)

                Button {
                    tuckService.refreshDrives()
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(tuckService.isLoading)
            }
        }
        .fileImporter(
            isPresented: $showingAddPicker,
            allowedContentTypes: [.item, .folder],
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case .success(let urls):
                if let url = urls.first {
                    Task.detached {
                        await tuckService.addFile(at: url.path)
                    }
                }
            case .failure(let error):
                tuckService.error = .init(title: "File Picker Error", message: error.localizedDescription)
            }
        }
        .confirmationDialog(
            "Restore \(selectedEntry.map { fileName($0.originalPath) } ?? "")?",
            isPresented: $showingRestoreConfirm
        ) {
            Button("Restore & Remove from Drive") {
                guard let entry = selectedEntry else { return }
                Task.detached {
                    await tuckService.restoreEntry(entry, keepArchive: false)
                }
                selectedEntry = nil
            }
            Button("Restore & Keep on Drive") {
                guard let entry = selectedEntry else { return }
                Task.detached {
                    await tuckService.restoreEntry(entry, keepArchive: true)
                }
                selectedEntry = nil
            }
            Button("Cancel", role: .cancel) {}
        }
        .overlay {
            if tuckService.isLoading, let progress = tuckService.currentProgress {
                ProgressOverlay(progress: progress)
            }
        }
        .alert(
            "Interrupted Operation",
            isPresented: Binding(
                get: { tuckService.pendingOperation != nil },
                set: { if !$0 { tuckService.pendingOperation = nil } }
            )
        ) {
            Button("Clean Up", role: .destructive) {
                tuckService.resolvePending()
            }
            Button("Ignore", role: .cancel) {
                tuckService.pendingOperation = nil
            }
        } message: {
            if let op = tuckService.pendingOperation {
                Text(pendingMessage(op))
            }
        }
        .alert(
            tuckService.error?.title ?? "Error",
            isPresented: Binding(
                get: { tuckService.error != nil },
                set: { if !$0 { tuckService.error = nil } }
            )
        ) {
            Button("OK") { tuckService.error = nil }
        } message: {
            Text(tuckService.error?.message ?? "")
        }
        .onAppear {
            tuckService.refreshDrives()
        }
        .onDrop(of: [.fileURL], isTargeted: nil) { providers in
            handleDrop(providers)
        }
    }

    private func handleDrop(_ providers: [NSItemProvider]) -> Bool {
        guard tuckService.selectedDrive != nil, !tuckService.isLoading else { return false }
        for provider in providers {
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier) { data, _ in
                guard let data = data as? Data,
                      let url = URL(dataRepresentation: data, relativeTo: nil) else { return }
                Task.detached {
                    await tuckService.addFile(at: url.path)
                }
            }
        }
        return true
    }
}

private func fileName(_ path: String) -> String {
    (path as NSString).lastPathComponent
}

private func pendingMessage(_ op: FfiPendingOperation) -> String {
    let kind: String
    switch op.kind {
    case .add: kind = "add"
    case .restore: kind = "restore"
    }
    let name = (op.originalPath as NSString).lastPathComponent
    return "A previous \(kind) operation for \"\(name)\" was interrupted. Clean up partial files to continue."
}
