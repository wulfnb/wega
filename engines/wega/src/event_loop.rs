//! Document-scoped task admission. V8 owns microtasks; this coordinator owns
//! when parser, resource, timer, lifecycle and rendering tasks may enter it.
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Task {
    Parser,
    Lifecycle,
    Timers,
    Resources,
}
#[derive(Default)]
pub(crate) struct Scheduler {
    revision: u64,
    tasks: VecDeque<(u64, Task)>,
    suspended: bool,
    frozen: bool,
}
impl Scheduler {
    pub fn replace(&mut self, revision: u64) {
        self.revision = revision;
        self.tasks.clear();
        self.suspended = false;
    }
    pub fn suspend(&mut self) {
        self.suspended = true;
        self.tasks.clear();
    }
    pub fn resume(&mut self) {
        self.suspended = false;
    }
    pub fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
    }
    pub fn script_allowed(&self) -> bool {
        !self.suspended
    }
    pub fn enqueue(&mut self, task: Task) {
        if !self.suspended
            && !self
                .tasks
                .iter()
                .any(|entry| *entry == (self.revision, task))
        {
            self.tasks.push_back((self.revision, task));
        }
    }
    pub fn next(&mut self) -> Option<Task> {
        if self.frozen || self.suspended {
            return None;
        }
        while let Some((revision, task)) = self.tasks.pop_front() {
            if revision == self.revision {
                return Some(task);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_discards_old_tasks_and_freeze_preserves_current_tasks() {
        let mut scheduler = Scheduler::default();
        scheduler.replace(1);
        scheduler.enqueue(Task::Timers);
        scheduler.enqueue(Task::Timers);
        scheduler.enqueue(Task::Resources);
        scheduler.suspend();
        assert!(!scheduler.script_allowed());
        assert_eq!(scheduler.next(), None);
        scheduler.replace(2);
        assert_eq!(scheduler.next(), None);
        scheduler.enqueue(Task::Parser);
        scheduler.enqueue(Task::Parser);
        scheduler.freeze(true);
        assert_eq!(scheduler.next(), None);
        scheduler.freeze(false);
        assert_eq!(scheduler.next(), Some(Task::Parser));
        assert_eq!(scheduler.next(), None);
    }
}
