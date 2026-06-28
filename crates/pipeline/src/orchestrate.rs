//! Sequencing the pipeline stages.

use std::future::Future;

use crate::error::PipelineError;
use crate::progress::ProgressReporter;
use crate::stage::PipelineStage;

/// Run the pipeline stages in order, stopping at the first failure.
///
/// Stages run in [`PipelineStage::ORDER`], with progress reported as each
/// begins. The work for each stage is supplied by `run_stage`, so the sequencing
/// stays independent of the concrete probe/detect/sync/assemble/export
/// implementations.
///
/// # Errors
///
/// [`PipelineError::Stage`] for the first stage whose `run_stage` returns `Err`,
/// carrying that stage and the underlying error as its source. Later stages are
/// not run.
pub async fn run_pipeline<F, Fut, E>(
    reporter: &ProgressReporter,
    mut run_stage: F,
) -> Result<(), PipelineError>
where
    F: FnMut(PipelineStage) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    for stage in PipelineStage::ORDER {
        reporter.enter(stage);
        if let Err(source) = run_stage(stage).await {
            reporter.fail(stage);
            return Err(PipelineError::Stage {
                stage,
                source: Box::new(source),
            });
        }
    }
    reporter.complete();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::fmt;

    use super::run_pipeline;
    use crate::error::PipelineError;
    use crate::progress::{ProgressReporter, RunProgress};
    use crate::stage::PipelineStage;

    #[derive(Debug)]
    struct StageBoom;

    impl fmt::Display for StageBoom {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("boom")
        }
    }

    impl std::error::Error for StageBoom {}

    #[tokio::test]
    async fn runs_every_stage_in_order_then_completes() {
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();
        let mut seen = Vec::new();

        let result = run_pipeline(&reporter, |stage| {
            seen.push(stage);
            async move { Ok::<(), Infallible>(()) }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(seen, PipelineStage::ORDER.to_vec());
        assert_eq!(subscription.current(), RunProgress::Completed);
    }

    #[tokio::test]
    async fn stops_at_the_first_failing_stage() {
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();
        let mut seen = Vec::new();

        let result = run_pipeline(&reporter, |stage| {
            seen.push(stage);
            async move {
                if stage == PipelineStage::Sync {
                    Err(StageBoom)
                } else {
                    Ok(())
                }
            }
        })
        .await;

        // Sync fails, so Assemble and Export never run.
        assert_eq!(
            seen,
            vec![
                PipelineStage::Probe,
                PipelineStage::Detect,
                PipelineStage::Sync,
            ]
        );
        assert!(matches!(
            result,
            Err(PipelineError::Stage {
                stage: PipelineStage::Sync,
                ..
            })
        ));
        assert_eq!(
            subscription.current(),
            RunProgress::Failed(PipelineStage::Sync)
        );
    }
}
