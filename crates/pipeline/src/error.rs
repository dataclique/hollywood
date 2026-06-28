//! Pipeline run errors.

use thiserror::Error;

use crate::stage::PipelineStage;

/// A pipeline run failure.
#[derive(Debug, Error)]
pub enum PipelineError {
    /// A stage's work failed; the run stopped at this stage. The underlying
    /// cause is preserved as the error source.
    #[error("{stage:?} stage failed")]
    Stage {
        /// The stage whose work returned an error.
        stage: PipelineStage,
        /// The failure the stage's work produced.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
