import Foundation
import SwiftUI

@Observable
class TuckService {
    var drives: [FfiDriveInfo] = []
    var selectedDrive: FfiDriveInfo?
    var entries: [FfiArchiveEntry] = []
    var isLoading = false
    var error: AppError?
    var currentProgress: ProgressReporter?
    var pendingOperation: FfiPendingOperation?

    struct AppError: Identifiable {
        let id = UUID()
        let title: String
        let message: String
    }

    func refreshDrives() {
        do {
            drives = try listDrives()
            // Auto-select if only one drive, or keep current selection
            if let current = selectedDrive,
               drives.contains(where: { $0.name == current.name }) {
                // Keep current selection
            } else if drives.count == 1 {
                selectedDrive = drives.first
            } else {
                selectedDrive = nil
            }
            if selectedDrive != nil {
                loadEntries()
            } else {
                entries = []
            }
        } catch {
            self.error = AppError(title: "Drive Error", message: describeError(error))
        }
    }

    func loadEntries() {
        guard let drive = selectedDrive else {
            entries = []
            pendingOperation = nil
            return
        }
        do {
            pendingOperation = try loadPending(driveRoot: drive.rootPath)
            entries = try loadManifestEntries(driveRoot: drive.rootPath)
        } catch {
            self.error = AppError(title: "Load Error", message: describeError(error))
        }
    }

    func resolvePending() {
        guard let drive = selectedDrive else { return }
        do {
            try cleanupPending(driveRoot: drive.rootPath)
            pendingOperation = nil
            loadEntries()
        } catch {
            self.error = AppError(title: "Cleanup Error", message: describeError(error))
        }
    }

    func addFile(at path: String) async {
        guard let drive = selectedDrive else {
            error = AppError(title: "No Drive", message: "Please connect and select a drive first.")
            return
        }

        let plan: FfiAddPlan
        do {
            plan = try planAdd(path: path, drive: drive)
        } catch {
            await MainActor.run {
                self.error = AppError(title: "Add Error", message: describeError(error))
            }
            return
        }

        let progress = ProgressReporter()
        await MainActor.run {
            self.currentProgress = progress
            self.isLoading = true
        }

        do {
            let _ = try executeAdd(plan: plan, progress: progress)
            try deleteLocal(path: plan.originalPath)
            await MainActor.run {
                self.currentProgress = nil
                self.isLoading = false
                self.loadEntries()
            }
        } catch {
            await MainActor.run {
                self.currentProgress = nil
                self.isLoading = false
                self.error = AppError(title: "Add Failed", message: describeError(error))
            }
        }
    }

    func restoreEntry(_ entry: FfiArchiveEntry, keepArchive: Bool) async {
        guard let drive = selectedDrive else { return }

        let plan: FfiRestorePlan
        do {
            plan = try planRestore(path: entry.originalPath, drive: drive)
        } catch {
            await MainActor.run {
                self.error = AppError(title: "Restore Error", message: describeError(error))
            }
            return
        }

        let progress = ProgressReporter()
        await MainActor.run {
            self.currentProgress = progress
            self.isLoading = true
        }

        do {
            try executeRestore(plan: plan, keepArchive: keepArchive, progress: progress)
            await MainActor.run {
                self.currentProgress = nil
                self.isLoading = false
                self.loadEntries()
            }
        } catch {
            await MainActor.run {
                self.currentProgress = nil
                self.isLoading = false
                self.error = AppError(title: "Restore Failed", message: describeError(error))
            }
        }
    }

    private func describeError(_ error: Error) -> String {
        if let ffiError = error as? FfiTuckError {
            return ffiError.localizedDescription
        }
        return error.localizedDescription
    }
}
