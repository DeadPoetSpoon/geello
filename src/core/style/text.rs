use serde::{Deserialize, Serialize};

/// Text label renderer configuration.
///
/// TODO: 1.7.5 — add font family, size, colour, anchor, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextConfig {}
