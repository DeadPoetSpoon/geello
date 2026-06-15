use std::collections::HashMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::core::error::{GeelloError, GeelloResult};
use crate::core::layer::PropertyValue;

/// How to interpret a raw string value from a property or remote source.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Encoding {
    /// Direct [`PropertyValue`]-to-`T` conversion via [`FromPropValue`].
    #[default]
    PropertyValue,
    /// Parse the string as a TOML document.
    Toml,
    /// Parse the string as JSON.
    Json,
}

/// A value that may originate from multiple sources and is evaluated lazily at render time.
///
/// # Variants
///
/// | Variant | Source | Requires |
/// |---------|--------|----------|
/// | `Inline(T)` | Direct value | — |
/// | `File(String)` | TOML file content | — |
/// | `Toml(String)` | Inline TOML string | — |
/// | `Prop { field, encoding }` | GeoJSON feature property | `properties` in [`ResolveContext`] |
/// | `JsonFile(String)` | JSON file content | — |
/// | `Url { url, encoding }` | HTTP response body | `from_http` + `http` in [`ResolveContext`] |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LazyValue<T> {
    /// Value provided directly inline in the style file.
    Inline(T),
    /// Path to an external TOML file whose content is the value.
    File(String),
    /// Inline TOML string to be parsed as the value.
    Toml(String),
    /// Read from a GeoJSON feature property.
    Prop {
        field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Encoding>,
    },
    /// Path to a JSON file whose content is the value.
    JsonFile(String),
    /// Fetch from an HTTP URL.
    #[cfg(feature = "from_http")]
    Url {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Encoding>,
    },
}

/// External data needed to resolve certain [`LazyValue`] variants.
///
/// All fields are optional — variants that don't need a particular
/// piece of context will simply ignore it.
pub struct ResolveContext<'a> {
    /// Feature properties for [`LazyValue::Prop`] resolution.
    pub properties: Option<&'a HashMap<String, PropertyValue>>,
}

impl<'a> Default for ResolveContext<'a> {
    fn default() -> Self {
        Self { properties: None }
    }
}

/// Types that can be constructed from a [`PropertyValue`] reference.
///
/// Used by [`LazyValue::resolve`] to convert GeoJSON property values
/// into the target type `T`.
pub trait FromPropValue: Sized {
    fn from_prop_value(value: &PropertyValue) -> Option<Self>;
}

impl FromPropValue for String {
    fn from_prop_value(value: &PropertyValue) -> Option<Self> {
        match value {
            PropertyValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl FromPropValue for f64 {
    fn from_prop_value(value: &PropertyValue) -> Option<Self> {
        match value {
            PropertyValue::Number(n) => Some(*n),
            _ => None,
        }
    }
}

impl FromPropValue for bool {
    fn from_prop_value(value: &PropertyValue) -> Option<Self> {
        match value {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl<T: DeserializeOwned> LazyValue<T> {
    /// Resolve self-contained variants (Inline, File, Toml, JsonFile, Url).
    ///
    /// For [`LazyValue::Prop`], returns an error — use [`LazyValue::resolve`]
    /// when property context is available.
    pub fn resolve_static(self) -> GeelloResult<T> {
        match self {
            LazyValue::Inline(v) => Ok(v),

            LazyValue::File(path) => {
                let raw = std::fs::read_to_string(&path)?;
                toml::from_str(&raw)
                    .map_err(GeelloError::TomlDe)
                    .or_else(|_| serde_json::from_str(&raw).map_err(GeelloError::Json))
            }

            LazyValue::Toml(content) => toml::from_str(&content)
                .map_err(GeelloError::TomlDe)
                .or_else(|_| serde_json::from_str(&content).map_err(GeelloError::Json)),

            LazyValue::Prop { .. } => Err(GeelloError::InvalidArgument(
                "LazyValue::Prop requires resolve(), not resolve_static()".into(),
            )),

            LazyValue::JsonFile(path) => {
                let raw = std::fs::read_to_string(&path)?;
                Ok(serde_json::from_str(&raw)?)
            }

            #[cfg(feature = "from_http")]
            LazyValue::Url { url, encoding } => {
                let response = reqwest::blocking::get(&url)?;
                let body = response.text()?;
                match encoding.unwrap_or_default() {
                    Encoding::PropertyValue | Encoding::Json => {
                        serde_json::from_str(&body).map_err(GeelloError::Json)
                    }
                    Encoding::Toml => toml::from_str(&body).map_err(GeelloError::TomlDe),
                }
            }
        }
    }
}

impl<T: DeserializeOwned + FromPropValue> LazyValue<T> {
    /// Evaluate the lazy value, producing a concrete `T`.
    ///
    /// Supports all variants including [`LazyValue::Prop`].
    /// Requires `T: FromPropValue` — for custom structs used with
    /// `Toml` / `Json` encoding, a trivial impl that returns `None`
    /// is sufficient (it will never be called for those encodings).
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<T> {
        match self {
            LazyValue::Prop { field, encoding } => {
                let props = ctx.properties.ok_or_else(|| {
                    GeelloError::GeoJson(
                        "LazyValue::Prop requires properties in ResolveContext".into(),
                    )
                })?;
                let val = props
                    .get(&field)
                    .ok_or_else(|| GeelloError::GeoJson(format!("Property '{field}' not found")))?;

                match encoding.unwrap_or_default() {
                    Encoding::PropertyValue => T::from_prop_value(val).ok_or_else(|| {
                        GeelloError::GeoJson(format!(
                            "Property '{field}' has incompatible type for target"
                        ))
                    }),
                    Encoding::Toml => {
                        let s = prop_as_string(val, &field)?;
                        toml::from_str(&s).map_err(GeelloError::TomlDe)
                    }
                    Encoding::Json => {
                        let s = prop_as_string(val, &field)?;
                        serde_json::from_str(&s).map_err(GeelloError::Json)
                    }
                }
            }
            other => other.resolve_static(),
        }
    }
}

/// Extract a string from a [`PropertyValue`] for TOML / JSON parsing.
fn prop_as_string(val: &PropertyValue, field: &str) -> GeelloResult<String> {
    match val {
        PropertyValue::String(s) => Ok(s.clone()),
        _ => Err(GeelloError::GeoJson(format!(
            "Property '{field}' must be a string for Toml/Json encoding, got {val:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("geello_test_lazy");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    // --- Serialisation round-trips ---

    #[test]
    fn test_serde_inline() {
        let v: LazyValue<f64> = LazyValue::Inline(3.14);
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<f64> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::Inline(x) if (x - 3.14).abs() < 1e-10));
    }

    #[test]
    fn test_serde_prop() {
        let v: LazyValue<String> = LazyValue::Prop {
            field: "name".into(),
            encoding: Some(Encoding::Toml),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<String> = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(back, LazyValue::Prop { field, encoding: Some(Encoding::Toml) }
                if field == "name")
        );
    }

    #[test]
    fn test_serde_file() {
        let v: LazyValue<i32> = LazyValue::File("params.toml".into());
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<i32> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::File(p) if p == "params.toml"));
    }

    // --- resolve() tests ---

    #[test]
    fn test_resolve_inline() {
        let v: LazyValue<f64> = LazyValue::Inline(2.72);
        assert!((v.resolve_static().unwrap() - 2.72).abs() < 1e-10);
    }

    #[test]
    fn test_resolve_toml_inline() {
        let v: LazyValue<f64> = LazyValue::Toml("3.14".into());
        assert!((v.resolve_static().unwrap() - 3.14).abs() < 1e-10);
    }

    #[test]
    fn test_resolve_file() {
        let path = temp_file("val.toml", "42");
        let v: LazyValue<i32> = LazyValue::File(path.to_string_lossy().into());
        assert_eq!(v.resolve_static().unwrap(), 42);
    }

    #[test]
    fn test_resolve_json_file() {
        let path = temp_file("val.json", "\"hello\"");
        let v: LazyValue<String> = LazyValue::JsonFile(path.to_string_lossy().into());
        assert_eq!(v.resolve_static().unwrap(), "hello");
    }

    #[test]
    fn test_resolve_prop_string() {
        let mut props = HashMap::new();
        props.insert("city".into(), PropertyValue::String("Beijing".into()));

        let v: LazyValue<String> = LazyValue::Prop {
            field: "city".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
            ..Default::default()
        };
        assert_eq!(v.resolve(&ctx).unwrap(), "Beijing");
    }

    #[test]
    fn test_resolve_prop_number() {
        let mut props = HashMap::new();
        props.insert("pop".into(), PropertyValue::Number(1_400_000_000.0));

        let v: LazyValue<f64> = LazyValue::Prop {
            field: "pop".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
            ..Default::default()
        };
        assert!((v.resolve(&ctx).unwrap() - 1_400_000_000.0).abs() < 1.0);
    }

    #[test]
    fn test_resolve_prop_bool() {
        let mut props = HashMap::new();
        props.insert("active".into(), PropertyValue::Bool(true));

        let v: LazyValue<bool> = LazyValue::Prop {
            field: "active".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
            ..Default::default()
        };
        assert!(v.resolve(&ctx).unwrap());
    }

    #[test]
    fn test_resolve_prop_not_found() {
        let props = HashMap::new();
        let v: LazyValue<String> = LazyValue::Prop {
            field: "missing".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
            ..Default::default()
        };
        assert!(v.resolve(&ctx).is_err());
    }

    #[test]
    fn test_resolve_prop_type_mismatch() {
        let mut props = HashMap::new();
        props.insert("name".into(), PropertyValue::Number(1.0)); // expect String

        let v: LazyValue<String> = LazyValue::Prop {
            field: "name".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
            ..Default::default()
        };
        assert!(v.resolve(&ctx).is_err());
    }

    #[test]
    fn test_resolve_prop_missing_context() {
        let v: LazyValue<String> = LazyValue::Prop {
            field: "x".into(),
            encoding: None,
        };
        let ctx = ResolveContext::default(); // no properties
        assert!(v.resolve(&ctx).is_err());
    }

    #[test]
    fn test_resolve_prop_encoding_json() {
        let mut props = HashMap::new();
        props.insert(
            "config".into(),
            PropertyValue::String("{\"name\":\"hello\"}".into()),
        );

        #[derive(Debug, Deserialize, PartialEq)]
        struct TestConfig {
            name: String,
        }
        // Trivial impl — never called, only satisfies resolve() bound.
        impl FromPropValue for TestConfig {
            fn from_prop_value(_value: &PropertyValue) -> Option<Self> {
                None
            }
        }

        let v: LazyValue<TestConfig> = LazyValue::Prop {
            field: "config".into(),
            encoding: Some(Encoding::Json),
        };
        let ctx = ResolveContext {
            properties: Some(&props),
        };
        assert_eq!(v.resolve(&ctx).unwrap().name, "hello");
    }
}
