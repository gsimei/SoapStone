//! One execution per request, reserved before preparation and retained until cancellation completes.
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Default)]
pub struct Runs(Mutex<Registry>);
#[derive(Default)]
struct Registry {
    entries: HashMap<i64, Entry>,
    cancelled: VecDeque<(i64, String)>,
}
struct Entry {
    token: String,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub request_id: i64,
    pub run_id: String,
}

pub struct RunGuard<'a> {
    runs: &'a Runs,
    request: i64,
    token: String,
}

impl Runs {
    pub fn start(&self, request: i64, token: String) -> Result<(RunGuard<'_>, oneshot::Receiver<()>), String> {
        if token.is_empty() || token.len() > 128 {
            return Err("Invalid execution identifier".into());
        }
        let mut registry = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = registry.cancelled.iter().position(|(id, t)| *id == request && t == &token) {
            registry.cancelled.remove(index);
            return Err("Request cancelled".into());
        }
        let entries = &mut registry.entries;
        if entries.contains_key(&request) {
            return Err("This request is already running. Wait or cancel its current execution.".into());
        }
        let (tx, rx) = oneshot::channel();
        entries.insert(request, Entry { token: token.clone(), cancel: Some(tx) });
        Ok((RunGuard { runs: self, request, token }, rx))
    }

    pub fn cancel(&self, request: i64, token: &str) {
        let mut registry = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = registry.entries.get_mut(&request).filter(|e| e.token == token)
            && let Some(tx) = entry.cancel.take()
        {
            let _ = tx.send(());
        } else if !registry.entries.contains_key(&request) {
            registry.cancelled.push_back((request, token.to_string()));
            if registry.cancelled.len() > 100 {
                registry.cancelled.pop_front();
            }
        }
    }

    pub fn active(&self) -> Vec<ActiveRun> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .iter()
            .map(|(&id, e)| ActiveRun { request_id: id, run_id: e.token.clone() })
            .collect()
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        let mut registry = self.runs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = &mut registry.entries;
        if entries.get(&self.request).is_some_and(|e| e.token == self.token) {
            entries.remove(&self.request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_cannot_release_or_cancel_another_execution() {
        let runs = Runs::default();
        let (guard, mut cancel) = runs.start(1, "first".into()).unwrap();
        assert!(runs.start(1, "second".into()).is_err());
        runs.cancel(1, "stale");
        assert_eq!(cancel.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        runs.cancel(1, "first");
        assert_eq!(cancel.try_recv(), Ok(()));
        assert!(runs.start(1, "second".into()).is_err());
        drop(guard);
        let (_guard, mut next) = runs.start(1, "second".into()).unwrap();
        runs.cancel(1, "first");
        assert_eq!(next.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    }

    #[test]
    fn cancellation_during_preparation_prevents_the_send_from_starting() {
        let runs = Runs::default();
        runs.cancel(1, "queued");
        assert!(runs.start(1, "queued".into()).is_err());
        assert!(runs.active().is_empty());
        assert!(runs.start(1, "next".into()).is_ok());
    }
}
