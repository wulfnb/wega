//! Document-bound native permission responses to the Wega worker.
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use wega::engine_protocol::{Command, authority::TrustedRequest};
pub(crate) struct Permission {
    pub sender: SyncSender<Command>,
    pub state: Arc<Mutex<crate::wega_page::Snapshot>>,
    pub requester: TrustedRequest,
    pub id: u64,
    pub feature: wega::engine_protocol::permissions::PermissionFeature,
}
impl wega::permissions::PermissionPort for Permission {
    fn respond(self, allow: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.accepts_requester(&self.requester) {
            if self
                .sender
                .try_send(Command::AnswerPermission { id: self.id, allow })
                .is_ok()
            {
                if allow {
                    state.grants.insert(self.feature);
                } else {
                    state.grants.remove(&self.feature);
                }
            }
        }
    }
}
