use serde::{Deserialize, Serialize};

/// Area / polygon renderer configuration.
///
/// TODO: 1.7.4 — add fill brush, optional stroke border, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AreaConfig {}
