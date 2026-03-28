import Foundation

final class ProgressReporter: FfiProgress, @unchecked Sendable {
    private let lock = NSLock()
    private var _phase: String = ""
    private var _totalBytes: UInt64 = 0
    private var _completedBytes: UInt64 = 0

    var phase: String {
        lock.lock()
        defer { lock.unlock() }
        return _phase
    }

    var totalBytes: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return _totalBytes
    }

    var completedBytes: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return _completedBytes
    }

    var fractionCompleted: Double {
        let total = totalBytes
        guard total > 0 else { return 0 }
        return Double(completedBytes) / Double(total)
    }

    func startPhase(phase: String, totalBytes: UInt64) {
        lock.lock()
        _phase = phase
        _totalBytes = totalBytes
        _completedBytes = 0
        lock.unlock()
    }

    func advance(bytes: UInt64) {
        lock.lock()
        _completedBytes += bytes
        lock.unlock()
    }

    func finishPhase() {
        lock.lock()
        _phase = ""
        _completedBytes = 0
        _totalBytes = 0
        lock.unlock()
    }
}
