//! The pipeline's ordered stages.

/// A stage of the pipeline, from raw footage to exported timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PipelineStage {
    /// Read each source's stream properties and decode its audio for analysis.
    Probe,
    /// Detect speech vs dead air to find the keep regions.
    Detect,
    /// Align audio captured by multiple sources.
    Sync,
    /// Assemble the trimmed, aligned timeline IR.
    Assemble,
    /// Serialize the timeline to the NLE export formats.
    Export,
}

impl PipelineStage {
    /// Every stage, in execution order.
    pub const ORDER: [Self; 5] = [
        Self::Probe,
        Self::Detect,
        Self::Sync,
        Self::Assemble,
        Self::Export,
    ];
}

#[cfg(test)]
mod tests {
    use super::PipelineStage;

    #[test]
    fn order_lists_every_stage_from_probe_to_export() {
        assert_eq!(PipelineStage::ORDER.len(), 5);
        assert_eq!(PipelineStage::ORDER.first(), Some(&PipelineStage::Probe));
        assert_eq!(PipelineStage::ORDER.last(), Some(&PipelineStage::Export));
    }

    #[test]
    fn order_has_no_repeats() {
        for (index, stage) in PipelineStage::ORDER.iter().enumerate() {
            assert!(!PipelineStage::ORDER[..index].contains(stage));
        }
    }
}
