use std::{collections::HashMap, convert::From};

use peniko::Brush;
use serde::{Deserialize, Serialize};
use vello::kurbo::Stroke;

/// Trait for values that can be resolved from GeoJSON feature properties.
pub trait MagicConverter {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String>;
}

impl MagicConverter for Brush {
    fn convert(&mut self, _: &HashMap<String, PropValue>) -> Result<(), String> {
        Ok(())
    }
}

impl MagicConverter for Stroke {
    fn convert(&mut self, _: &HashMap<String, PropValue>) -> Result<(), String> {
        Ok(())
    }
}

/// Trait for values that can be fetched from external sources (files, URLs).
pub trait MagicFetcher {
    fn fetch(&mut self) -> Result<(), String>;
}

impl MagicFetcher for Brush {
    fn fetch(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl MagicFetcher for Stroke {
    fn fetch(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Serialization encoding format for external value sources.
#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum StrEncoding {
    #[default]
    Toml,
    #[cfg(feature = "from_json")]
    Json,
}

/// Describes where a lazy value should be resolved from.
#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
pub enum ValueSource {
    /// Value is inlined in the configuration — no resolution needed.
    #[default]
    Inline,
    /// Read value from a GeoJSON feature property field.
    /// The field value is parsed according to the specified encoding.
    Prop(String, StrEncoding),
    /// Load value from a TOML file on disk.
    File(String),
    /// Load value from a JSON file on disk (requires `from_json` feature).
    #[cfg(feature = "from_json")]
    JsonFile(String),
    /// Fetch value from an HTTP URL (requires `from_http` feature).
    #[cfg(feature = "from_http")]
    Url(String, StrEncoding),
}

/// A lazily-resolved value container.
///
/// `LazyValue<T>` wraps a value and supports deferred resolution from
/// multiple sources:
/// - **Inline**: value is defined directly in configuration
/// - **File**: loaded from a TOML/JSON file on disk
/// - **Url**: fetched from an HTTP endpoint (feature `from_http`)
/// - **Prop**: read from a GeoJSON feature's `properties` field
///
/// # TOML Serialization
///
/// When `kind` is `Inline` (default), the value serializes as the inner
/// type directly — no wrapper. When `kind` is a non-default source
/// (File/Url/Prop), the value serializes with explicit `kind` and
/// `need_scale` fields alongside the inner value's fields.
#[derive(Debug, Clone)]
pub struct LazyValue<T> {
    inner: T,
    kind: ValueSource,
    need_scale: bool,
}

// ── Serialize ──

impl<T: Serialize> Serialize for LazyValue<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.kind == ValueSource::Inline && !self.need_scale {
            // Inline: serialize just the inner value directly
            self.inner.serialize(serializer)
        } else {
            // Full: serialize as a map with inner fields merged with kind/need_scale
            let value = toml::Value::try_from(&self.inner).map_err(serde::ser::Error::custom)?;
            match value {
                toml::Value::Table(mut map) => {
                    if self.kind != ValueSource::Inline {
                        let kind_val =
                            toml::Value::try_from(&self.kind).map_err(serde::ser::Error::custom)?;
                        map.insert("kind".to_string(), kind_val);
                    }
                    if self.need_scale {
                        map.insert("need_scale".to_string(), toml::Value::Boolean(true));
                    }
                    map.serialize(serializer)
                },
                _ => {
                    // Scalar inner value with extra fields: serialize as { value, kind, need_scale }
                    use serde::ser::SerializeMap;
                    let mut map = serializer.serialize_map(Some(3))?;
                    map.serialize_entry("value", &self.inner)?;
                    if self.kind != ValueSource::Inline {
                        map.serialize_entry("kind", &self.kind)?;
                    }
                    if self.need_scale {
                        map.serialize_entry("need_scale", &self.need_scale)?;
                    }
                    map.end()
                },
            }
        }
    }
}

// ── Deserialize ──

impl<'de, T: Deserialize<'de>> Deserialize<'de> for LazyValue<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        // Deserialize into a toml::Value to inspect the structure
        let raw = toml::Value::deserialize(deserializer)?;

        match raw {
            toml::Value::Table(mut table) => {
                // Check if this table has LazyValue metadata fields
                let kind = if let Some(kind_val) = table.remove("kind") {
                    ValueSource::deserialize(kind_val).map_err(Error::custom)?
                } else {
                    ValueSource::Inline
                };
                let need_scale = if let Some(ns) = table.remove("need_scale") {
                    bool::deserialize(ns).map_err(Error::custom)?
                } else {
                    false
                };

                // The remaining table fields are the inner value
                let inner = T::deserialize(toml::Value::Table(table)).map_err(Error::custom)?;
                Ok(LazyValue {
                    inner,
                    kind,
                    need_scale,
                })
            },
            other => {
                // Scalar or array: inline value only
                let inner = T::deserialize(other).map_err(Error::custom)?;
                Ok(LazyValue {
                    inner,
                    kind: ValueSource::Inline,
                    need_scale: false,
                })
            },
        }
    }
}

impl<T> LazyValue<T>
where
    T: Default,
{
    /// Create a LazyValue pointing to a TOML file.
    pub fn new_file(path: String) -> Self {
        Self {
            inner: Default::default(),
            kind: ValueSource::File(path),
            need_scale: false,
        }
    }
}

impl<T> LazyValue<T> {
    /// Create a LazyValue with an inlined value.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            kind: ValueSource::Inline,
            need_scale: false,
        }
    }

    /// Consume and return the inner value.
    pub fn unwrap(self) -> T {
        self.inner
    }

    /// Get a shared reference to the inner value.
    pub fn as_ref(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner value.
    pub fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Default for LazyValue<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            inner: Default::default(),
            kind: ValueSource::Inline,
            need_scale: false,
        }
    }
}

impl<T> From<T> for LazyValue<T> {
    fn from(value: T) -> Self {
        LazyValue::new(value)
    }
}

impl LazyValue<PropValue> {
    /// Try to convert the inner PropValue into type T.
    pub fn inner_try_into<T: TryFrom<PropValue, Error = std::string::String>>(
        &self,
    ) -> Result<T, String> {
        self.inner.clone().try_into()
    }

    /// Get the string representation of the inner value.
    pub fn to_string(&self) -> String {
        self.inner.clone().to_string()
    }

    /// Wrap a value into a LazyValue<PropValue>.
    pub fn wrap<D: Into<PropValue>>(value: D) -> Self {
        Self {
            inner: Into::<PropValue>::into(value),
            kind: ValueSource::Inline,
            need_scale: false,
        }
    }
}

impl<T> LazyValue<T>
where
    T: for<'de> Deserialize<'de> + Default + MagicFetcher + MagicConverter,
{
    /// Resolve a Prop-sourced value from GeoJSON feature properties.
    pub fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        match &self.kind {
            ValueSource::Prop(name, encoding) => {
                if let Some(value) = props.get(name) {
                    let mut v: LazyValue<T> = match encoding {
                        StrEncoding::Toml => toml::from_str(&value.to_string()).map_err(|e| {
                            format!("Deserializing value from Prop:{} error: {}", name, e)
                        })?,
                        #[cfg(feature = "from_json")]
                        StrEncoding::Json => {
                            serde_json::from_str(&value.to_string()).map_err(|e| {
                                format!("Deserializing value from Prop:{} error: {}", name, e)
                            })?
                        },
                    };
                    v.fetch()?;
                    v.convert(props)?;
                    self.inner = v.unwrap();
                } else {
                    return Err(format!("No {} found in props", name));
                }
            },
            _ => {},
        }
        self.inner.fetch()?;
        self.inner.convert(props)?;
        Ok(())
    }
}

impl<T> LazyValue<T>
where
    T: for<'de> Deserialize<'de> + Default + MagicFetcher,
{
    /// Fetch the value from its external source (file or URL).
    pub fn fetch(&mut self) -> Result<(), String> {
        let inner = match &self.kind {
            ValueSource::File(path) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Read value from File:{} error: {}", path, e))?;
                let mut value: LazyValue<T> = toml::from_str(&content)
                    .map_err(|e| format!("Deserializing value from File:{} error: {}", path, e))?;
                value.fetch()?;
                Some(value.unwrap())
            },
            #[cfg(feature = "from_json")]
            ValueSource::JsonFile(path) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Read value from File:{} error: {}", path, e))?;
                let mut value: LazyValue<T> = serde_json::from_str(&content)
                    .map_err(|e| format!("Deserializing value from File:{} error: {}", path, e))?;
                value.fetch()?;
                Some(value.unwrap())
            },
            #[cfg(feature = "from_http")]
            ValueSource::Url(url, encoding) => {
                let res = reqwest::blocking::get(url)
                    .map_err(|e| format!("Get value from Url:{} error: {}", url, e))?;
                let text = res
                    .text()
                    .map_err(|e| format!("Get text from Url:{} error: {}", url, e))?;
                let mut value: LazyValue<T> = match encoding {
                    StrEncoding::Toml => toml::from_str(&text).map_err(|e| {
                        format!("Deserializing value from Url:{} error: {}", url, e)
                    })?,
                    #[cfg(feature = "from_json")]
                    StrEncoding::Json => serde_json::from_str(&text).map_err(|e| {
                        format!("Deserializing value from Url:{} error: {}", url, e)
                    })?,
                };
                value.fetch()?;
                Some(value.unwrap())
            },
            _ => None,
        };
        if inner.is_some() {
            self.inner = inner.unwrap();
        }
        self.inner.fetch()?;
        Ok(())
    }
}

// ── PropValue ──

/// A typed property value extracted from GeoJSON feature properties.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum PropValue {
    #[default]
    None,
    String(String),
    Float64(f64),
    Float32(f32),
    Int32(i32),
    Int64(i64),
    Boolean(bool),
}

impl MagicFetcher for PropValue {
    fn fetch(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl MagicConverter for PropValue {
    fn convert(&mut self, _: &HashMap<String, PropValue>) -> Result<(), String> {
        Ok(())
    }
}

impl From<&str> for PropValue {
    fn from(value: &str) -> Self {
        PropValue::String(value.to_string())
    }
}

impl From<String> for PropValue {
    fn from(value: String) -> Self {
        PropValue::String(value)
    }
}

impl From<f32> for PropValue {
    fn from(value: f32) -> Self {
        PropValue::Float32(value)
    }
}

impl From<f64> for PropValue {
    fn from(value: f64) -> Self {
        PropValue::Float64(value)
    }
}

impl From<bool> for PropValue {
    fn from(value: bool) -> Self {
        PropValue::Boolean(value)
    }
}

impl ToString for PropValue {
    fn to_string(&self) -> String {
        match self {
            PropValue::String(v) => v.clone(),
            PropValue::Float64(v) => v.to_string(),
            PropValue::Float32(v) => v.to_string(),
            PropValue::Int32(v) => v.to_string(),
            PropValue::Int64(v) => v.to_string(),
            PropValue::Boolean(v) => v.to_string(),
            PropValue::None => "None".to_string(),
        }
    }
}

impl TryFrom<PropValue> for String {
    type Error = String;

    fn try_from(value: PropValue) -> Result<Self, Self::Error> {
        match value {
            PropValue::String(v) => Ok(v),
            PropValue::Float64(v) => Ok(v.to_string()),
            PropValue::Float32(v) => Ok(v.to_string()),
            PropValue::Int32(v) => Ok(v.to_string()),
            PropValue::Int64(v) => Ok(v.to_string()),
            PropValue::Boolean(v) => Ok(v.to_string()),
            PropValue::None => Err("Cannot convert None to String".to_string()),
        }
    }
}

impl TryFrom<PropValue> for f32 {
    type Error = String;
    fn try_from(value: PropValue) -> Result<Self, Self::Error> {
        match value {
            PropValue::String(v) => v.parse().map_err(|e: std::num::ParseFloatError| {
                format!("Convert from string error: {}", e.to_string())
            }),
            PropValue::Float64(v) => Ok(v as f32),
            PropValue::Float32(v) => Ok(v),
            PropValue::Int32(v) => Ok(v as f32),
            PropValue::Int64(v) => Ok(v as f32),
            PropValue::Boolean(v) => {
                if v {
                    Ok(1.0)
                } else {
                    Ok(0.0)
                }
            },
            PropValue::None => Err("Cannot convert None to f32".to_string()),
        }
    }
}

impl TryFrom<PropValue> for f64 {
    type Error = String;

    fn try_from(value: PropValue) -> Result<Self, Self::Error> {
        match value {
            PropValue::String(v) => v.parse().map_err(|e: std::num::ParseFloatError| {
                format!("Convert from string error: {}", e.to_string())
            }),
            PropValue::Float64(v) => Ok(v),
            PropValue::Float32(v) => Ok(v as f64),
            PropValue::Int32(v) => Ok(v as f64),
            PropValue::Int64(v) => Ok(v as f64),
            PropValue::Boolean(v) => {
                if v {
                    Ok(1.0)
                } else {
                    Ok(0.0)
                }
            },
            PropValue::None => Err("Cannot convert None to f64".to_string()),
        }
    }
}

impl TryFrom<PropValue> for bool {
    type Error = String;

    fn try_from(value: PropValue) -> Result<Self, Self::Error> {
        match value {
            PropValue::String(v) => v.parse().map_err(|e: std::str::ParseBoolError| {
                format!("Convert from string error: {}", e.to_string())
            }),
            PropValue::Float64(v) => Ok(v != 0.0),
            PropValue::Float32(v) => Ok(v != 0.0),
            PropValue::Int32(v) => Ok(v != 0),
            PropValue::Int64(v) => Ok(v != 0),
            PropValue::Boolean(v) => Ok(v),
            PropValue::None => Err("Cannot convert None to bool".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_value_string_inline() {
        // v = "hello" → LazyValue<String>
        let m: std::collections::HashMap<String, LazyValue<String>> =
            toml::from_str("v = \"hello\"").unwrap();
        let lv = &m["v"];
        assert_eq!(lv.as_ref(), "hello");
        assert_eq!(lv.kind, ValueSource::Inline);
    }

    #[test]
    fn lazy_value_table_inline() {
        // v = { x = 42 } → LazyValue<Foo>
        #[derive(Debug, Deserialize, PartialEq)]
        struct Foo {
            x: i32,
        }
        let m: std::collections::HashMap<String, LazyValue<Foo>> =
            toml::from_str("v = { x = 42 }").unwrap();
        let lv = &m["v"];
        assert_eq!(lv.as_ref().x, 42);
        assert_eq!(lv.kind, ValueSource::Inline);
    }

    #[test]
    fn lazy_value_with_kind() {
        // Table with kind field → LazyValue with external source
        let toml_str = r#"
[v]
kind = { File = "some/path.toml" }
x = 10
"#;
        #[derive(Debug, Default, Deserialize, PartialEq)]
        struct Foo {
            x: i32,
        }
        let m: std::collections::HashMap<String, LazyValue<Foo>> =
            toml::from_str(toml_str).unwrap();
        let lv = &m["v"];
        assert_eq!(lv.as_ref().x, 10);
        assert_eq!(lv.kind, ValueSource::File("some/path.toml".into()));
    }

    #[test]
    fn lazy_value_filter_none_roundtrip() {
        use crate::core::filter::RenderedGeometryFilter;
        // v = "None" → LazyValue<RenderedGeometryFilter>
        let m: std::collections::HashMap<String, LazyValue<RenderedGeometryFilter>> =
            toml::from_str("v = \"None\"").unwrap();
        let lv = &m["v"];
        assert_eq!(lv.kind, ValueSource::Inline);
        match lv.as_ref() {
            RenderedGeometryFilter::None => {},
            _ => panic!("Expected None"),
        }
    }

    #[test]
    fn lazy_value_filter_layer_roundtrip() {
        use crate::core::filter::RenderedGeometryFilter;
        // v = { Layer = "china" } → LazyValue<RenderedGeometryFilter>
        let m: std::collections::HashMap<String, LazyValue<RenderedGeometryFilter>> =
            toml::from_str("v = { Layer = \"china\" }").unwrap();
        let lv = &m["v"];
        assert_eq!(lv.kind, ValueSource::Inline);
        match lv.as_ref() {
            RenderedGeometryFilter::Layer(name) => assert_eq!(name, "china"),
            _ => panic!("Expected Layer"),
        }
    }
}
