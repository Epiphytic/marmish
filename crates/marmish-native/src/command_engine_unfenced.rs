//! Stock workers have no registered candidate default or fatal signal.
#[derive(Clone, Default)]
pub(super) struct Mode {}

impl Mode {
    pub(super) fn candidate(&self) -> bool {
        false
    }

    pub(super) fn connection_fence(&self) -> Option<marmish_engine::ConnectionFence> {
        None
    }

    pub(super) fn tripped(&self) -> bool {
        false
    }

    pub(super) fn admit(&self) -> bool {
        true
    }
}
