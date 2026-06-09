use crate::core::value::{MagicConverter, MagicFetcher, PropValue};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum RenderedGeometryFilter {
    #[default]
    None,
    Layer(String),
}

impl MagicFetcher for RenderedGeometryFilter {
    fn fetch(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl MagicConverter for RenderedGeometryFilter {
    fn convert(
        &mut self,
        _: &std::collections::HashMap<String, PropValue>,
    ) -> Result<(), String> {
        Ok(())
    }
}
