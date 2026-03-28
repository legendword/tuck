#[uniffi::export(callback_interface)]
pub trait FfiProgress: Send + Sync {
    fn start_phase(&self, phase: String, total_bytes: u64);
    fn advance(&self, bytes: u64);
    fn finish_phase(&self);
}

/// Bridge that implements tuck-core's Progress trait by delegating to an FfiProgress callback.
pub(crate) struct ProgressBridge {
    pub inner: Box<dyn FfiProgress>,
}

impl tuck_core::progress::Progress for ProgressBridge {
    fn start_phase(&self, phase: &str, total_bytes: u64) {
        self.inner.start_phase(phase.to_string(), total_bytes);
    }

    fn advance(&self, bytes: u64) {
        self.inner.advance(bytes);
    }

    fn finish_phase(&self) {
        self.inner.finish_phase();
    }
}
