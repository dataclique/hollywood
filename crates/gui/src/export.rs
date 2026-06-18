//! NLE export targets the user can enable for a run.

/// An interchange format Hollywood can emit once the pipeline is wired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExportTarget {
    /// FCP7 `xmeml` — Premiere Pro and DaVinci Resolve.
    Xmeml,
    /// FCPXML — Final Cut Pro and Resolve (not implemented yet).
    Fcpxml,
}

impl ExportTarget {
    /// Short label for the checkbox.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Xmeml => "Premiere / Resolve (FCP7 xmeml)",
            Self::Fcpxml => "Final Cut / Resolve (FCPXML)",
        }
    }

    /// Whether the exporter exists today.
    pub(crate) fn is_available(self) -> bool {
        matches!(self, Self::Xmeml)
    }

    /// Every target the UI offers.
    pub(crate) fn all() -> [Self; 2] {
        [Self::Xmeml, Self::Fcpxml]
    }
}

/// Which export formats are enabled for the next run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExportSelection {
    enabled: Vec<ExportTarget>,
}

impl ExportSelection {
    /// Xmeml only — the only exporter implemented so far.
    pub(crate) fn default_enabled() -> Self {
        Self {
            enabled: vec![ExportTarget::Xmeml],
        }
    }

    /// Whether `target` is checked.
    pub(crate) fn contains(&self, target: ExportTarget) -> bool {
        self.enabled.contains(&target)
    }

    /// Toggle a target on or off. Unavailable targets stay off.
    pub(crate) fn set(&mut self, target: ExportTarget, on: bool) {
        if on && target.is_available() {
            if !self.enabled.contains(&target) {
                self.enabled.push(target);
            }
        } else {
            self.enabled.retain(|t| *t != target);
        }
    }

    /// Whether any implemented export format is selected.
    pub(crate) fn has_implemented_target(&self) -> bool {
        self.enabled.iter().any(|t| t.is_available())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_target() {
        assert_eq!(
            ExportTarget::all(),
            [ExportTarget::Xmeml, ExportTarget::Fcpxml]
        );
    }

    #[test]
    fn only_xmeml_is_available_today() {
        assert!(ExportTarget::Xmeml.is_available());
        assert!(!ExportTarget::Fcpxml.is_available());
    }

    #[test]
    fn default_enables_only_xmeml() {
        let selection = ExportSelection::default_enabled();
        assert!(selection.contains(ExportTarget::Xmeml));
        assert!(!selection.contains(ExportTarget::Fcpxml));
        assert!(selection.has_implemented_target());
    }

    #[test]
    fn deselecting_xmeml_leaves_no_implemented_target() {
        let mut selection = ExportSelection::default_enabled();
        selection.set(ExportTarget::Xmeml, false);
        assert!(!selection.contains(ExportTarget::Xmeml));
        assert!(!selection.has_implemented_target());
    }

    #[test]
    fn unavailable_target_cannot_be_enabled() {
        // Toggling an unimplemented target on is a no-op — the invariant that
        // keeps the UI from offering an export Hollywood cannot produce.
        let mut selection = ExportSelection::default();
        selection.set(ExportTarget::Fcpxml, true);
        assert!(!selection.contains(ExportTarget::Fcpxml));
        assert!(!selection.has_implemented_target());
    }

    #[test]
    fn enabling_twice_does_not_duplicate() {
        let mut selection = ExportSelection::default();
        selection.set(ExportTarget::Xmeml, true);
        selection.set(ExportTarget::Xmeml, true);
        assert!(selection.contains(ExportTarget::Xmeml));
        selection.set(ExportTarget::Xmeml, false);
        assert!(!selection.contains(ExportTarget::Xmeml));
    }
}
