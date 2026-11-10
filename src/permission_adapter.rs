//! Document-bound native permission responses to the Wega worker.
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use wega::engine_protocol::{Command, authority::TrustedRequest};
pub(crate) struct Permission {
    pub sender: SyncSender<Command>,
    pub state: Arc<Mutex<crate::wega_page::Snapshot>>,
    pub requester: TrustedRequest,
    pub id: u64,
}
impl wega::permissions::PermissionPort for Permission {
    fn respond(self, allow: bool) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.accepts_requester(&self.requester) {
            let _ = self
                .sender
                .try_send(Command::AnswerPermission { id: self.id, allow });
        }
    }
}
