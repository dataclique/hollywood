//! A pipeline run's own progress channel.
//!
//! Per [ADR 0004](../adrs/0004-apalis-pipeline.md) progress is Hollywood's own
//! signal — apalis tracks job state, not percent — published over a
//! [`tokio::sync::watch`] channel so subscribers always read the latest state
//! without buffering intermediate updates.

use tokio::sync::watch;

use crate::stage::PipelineStage;

/// How far a pipeline run has progressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunProgress {
    /// Not yet started.
    Queued,
    /// Currently executing `stage`.
    Running(PipelineStage),
    /// Every stage completed.
    Completed,
    /// `stage` failed and the run stopped there.
    Failed(PipelineStage),
}

/// Publishes a run's [`RunProgress`] updates to its subscribers.
pub struct ProgressReporter {
    sender: watch::Sender<RunProgress>,
}

impl ProgressReporter {
    /// A reporter for a run that has not started ([`RunProgress::Queued`]).
    pub fn new() -> Self {
        let (sender, _) = watch::channel(RunProgress::Queued);
        Self { sender }
    }

    /// A subscription observing this run's progress, starting from the latest
    /// reported state.
    pub fn subscribe(&self) -> ProgressSubscription {
        ProgressSubscription {
            receiver: self.sender.subscribe(),
        }
    }

    /// Report that `stage` is now executing.
    pub fn enter(&self, stage: PipelineStage) {
        self.publish(RunProgress::Running(stage));
    }

    /// Report that the run finished successfully.
    pub fn complete(&self) {
        self.publish(RunProgress::Completed);
    }

    /// Report that `stage` failed and the run stopped.
    pub fn fail(&self, stage: PipelineStage) {
        self.publish(RunProgress::Failed(stage));
    }

    fn publish(&self, progress: RunProgress) {
        // The run proceeds even with no listeners; watch retains the latest value
        // for any subscriber that attaches later, so a send with no receivers is
        // not an error worth surfacing.
        let _ = self.sender.send(progress);
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Observes a pipeline run's [`RunProgress`].
pub struct ProgressSubscription {
    receiver: watch::Receiver<RunProgress>,
}

impl ProgressSubscription {
    /// The latest reported progress.
    pub fn current(&self) -> RunProgress {
        *self.receiver.borrow()
    }

    /// Wait for the next progress change and return it, or `None` once the
    /// reporter has been dropped and no further updates can arrive.
    pub async fn changed(&mut self) -> Option<RunProgress> {
        self.receiver.changed().await.ok()?;
        Some(*self.receiver.borrow())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgressReporter, RunProgress};
    use crate::stage::PipelineStage;

    #[test]
    fn a_fresh_run_is_queued() {
        let reporter = ProgressReporter::new();
        assert_eq!(reporter.subscribe().current(), RunProgress::Queued);
    }

    #[test]
    fn subscribers_read_the_latest_reported_state() {
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();

        reporter.enter(PipelineStage::Detect);
        assert_eq!(
            subscription.current(),
            RunProgress::Running(PipelineStage::Detect)
        );

        reporter.complete();
        assert_eq!(subscription.current(), RunProgress::Completed);
    }

    #[tokio::test]
    async fn changed_yields_each_update_then_none_after_drop() {
        let reporter = ProgressReporter::new();
        let mut subscription = reporter.subscribe();

        reporter.fail(PipelineStage::Sync);
        assert_eq!(
            subscription.changed().await,
            Some(RunProgress::Failed(PipelineStage::Sync))
        );

        drop(reporter);
        assert_eq!(subscription.changed().await, None);
    }
}
