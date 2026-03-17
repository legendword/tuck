/// Trait for reporting progress of long-running operations.
/// All methods take `&self` — implementations should use interior mutability.
pub trait Progress {
    /// A new phase of the operation is starting (e.g. "Hashing source", "Copying").
    fn start_phase(&self, phase: &str, total_bytes: u64);
    /// Advance progress by the given number of bytes.
    fn advance(&self, bytes: u64);
    /// The current phase has finished.
    fn finish_phase(&self);
}
