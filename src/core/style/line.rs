use serde::{Deserialize, Serialize};

/// Line renderer configuration.
///
/// TODO: 1.7.3 — add stroke width, join, cap, colour, dash pattern, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineConfig {}
