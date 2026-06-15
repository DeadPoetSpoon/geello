use std::collections::HashMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::core::error::{GeelloError, GeelloResult};
use crate::core::layer::PropertyValue;

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// How to interpret a raw string value from a property or remote source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Encoding {
    /// Direct [`PropertyValue`]-to-`T` conversion via [`FromPropValue`].
    #[default]
    PropertyValue,
    /// Parse the string as a TOML document.
    Toml,
    /// Parse the string as JSON.
    Json,
}

// ---------------------------------------------------------------------------
// LazyValue — untagged so that bare scalars are `Inline`
// ---------------------------------------------------------------------------

/// A value that may originate from multiple sources and is evaluated lazily at render time.
///
/// # TOML representation (untagged)
///
/// | TOML | Variant |
/// |------|---------|
/// | `0.1`, `"red"`, `true` | `Inline(T)` |
/// | `{ file = "path.toml" }` | `File` |
/// | `{ toml = "key = 'value'" }` | `Toml` |
/// | `{ field = "name" }` | `Prop` (default encoding) |
/// | `{ field = "name", encoding = "Json" }` | `Prop` with encoding |
/// | `{ json = '"hello"' }` | `Json` |
/// | `{ url = "https://..." }` | `Url` |
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LazyValue<T> {
    /// `{ file = "path.toml" }`  or  `{ file = "path.json", encoding = "Json" }`
    File {
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Encoding>,
    },
    /// `{ toml = "key = 'value'" }`
    Toml { toml: String },
    /// `{ field = "name" }`  or  `{ field = "name", encoding = "Json" }`
    Prop {
        field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Encoding>,
    },
    /// `{ json = "data.json" }`
    Json { json: String },
    /// `{ url = "https://...", encoding = "Toml" }`
    #[cfg(feature = "from_http")]
    Url {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<Encoding>,
    },
    /// Bare value — `0.1`, `"hello"`, `true`, …
    Inline(T),
}

impl<T: Default> Default for LazyValue<T> {
    fn default() -> Self {
        LazyValue::Inline(T::default())
    }
}

/// Implement [`FromPropValue`] for a type that never originates from a
/// GeoJSON property.  The `from_prop_value` method always returns `None`.
///
/// This is a convenience for structural types (e.g. `LayerFilter`,
/// `Vec<LayerStyle>`) that need the bound to call [`LazyValue::resolve`]
/// but will never actually encounter a [`LazyValue::Prop`] variant.
#[macro_export]
macro_rules! from_prop_never {
    ($ty:ty) => {
        impl $crate::core::lazy::FromPropValue for $ty {
            fn from_prop_value(_value: &$crate::core::layer::PropertyValue) -> Option<Self> {
                None
            }
        }
    };
}

// ---------------------------------------------------------------------------
// ResolveContext
// ---------------------------------------------------------------------------

/// External data needed to resolve certain [`LazyValue`] variants.
pub struct ResolveContext<'a> {
    /// Feature properties for [`LazyValue::Prop`] resolution.
    pub properties: Option<&'a HashMap<String, PropertyValue>>,
}

impl<'a> Default for ResolveContext<'a> {
    fn default() -> Self {
        Self { properties: None }
    }
}

// ---------------------------------------------------------------------------
// FromPropValue
// ---------------------------------------------------------------------------

/// Types that can be constructed from a [`PropertyValue`] reference.
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

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

impl<T: DeserializeOwned + FromPropValue> LazyValue<T> {
    /// Evaluate the lazy value, producing a concrete `T`.
    ///
    /// The `ctx` is only consumed by [`LazyValue::Prop`]; all other variants
    /// ignore it.  Types that never use `Prop` can implement
    /// [`FromPropValue`] with [`from_prop_never!`] to satisfy the bound.
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<T> {
        match self {
            LazyValue::Inline(v) => Ok(v),

            LazyValue::File { file, encoding } => {
                let raw = std::fs::read_to_string(&file)?;
                match encoding.unwrap_or(Encoding::Toml) {
                    Encoding::Toml => toml::from_str(&raw).map_err(GeelloError::TomlDe),
                    Encoding::Json => serde_json::from_str(&raw).map_err(GeelloError::Json),
                    Encoding::PropertyValue => Err(GeelloError::InvalidArgument(
                        "Encoding::PropertyValue is not valid for File variant".into(),
                    )),
                }
            }

            LazyValue::Toml { toml } => toml::from_str(&toml)
                .map_err(GeelloError::TomlDe)
                .or_else(|_| serde_json::from_str(&toml).map_err(GeelloError::Json)),

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

            LazyValue::Json { json } => serde_json::from_str(&json).map_err(GeelloError::Json),

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

fn prop_as_string(val: &PropertyValue, field: &str) -> GeelloResult<String> {
    match val {
        PropertyValue::String(s) => Ok(s.clone()),
        _ => Err(GeelloError::GeoJson(format!(
            "Property '{field}' must be a string for Toml/Json encoding, got {val:?}"
        ))),
    }
}

/// Resolve an optional [`LazyValue`] — `None` maps to `Ok(None)`.
pub fn opt_resolve<T: DeserializeOwned + FromPropValue>(
    opt: Option<LazyValue<T>>,
    ctx: &ResolveContext,
) -> GeelloResult<Option<T>> {
    opt.map(|v| v.resolve(ctx)).transpose()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    // --- serde round-trips ---

    #[test]
    fn test_serde_inline() {
        let v: LazyValue<f64> = LazyValue::Inline(3.14);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "3.14");
        let back: LazyValue<f64> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::Inline(x) if (x - 3.14).abs() < 1e-10));
    }

    #[test]
    fn test_serde_file() {
        let v: LazyValue<i32> = LazyValue::File {
            file: "params.toml".into(),
            encoding: Some(Encoding::Toml),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<i32> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::File { file, .. } if file == "params.toml"));
    }

    #[test]
    fn test_serde_file_default_encoding() {
        // encoding is optional — default None
        let v: LazyValue<i32> = LazyValue::File {
            file: "params.toml".into(),
            encoding: None,
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<i32> = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(back, LazyValue::File { file, encoding: None, .. } if file == "params.toml")
        );
    }

    #[test]
    fn test_serde_toml_str() {
        let v: LazyValue<f64> = LazyValue::Toml {
            toml: "3.14".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<f64> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::Toml { toml } if toml == "3.14"));
    }

    #[test]
    fn test_serde_prop() {
        let v: LazyValue<String> = LazyValue::Prop {
            field: "name".into(),
            encoding: Some(Encoding::Toml),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<String> = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            LazyValue::Prop { field, encoding: Some(Encoding::Toml) } if field == "name"
        ));
    }

    #[test]
    fn test_serde_json() {
        let v: LazyValue<String> = LazyValue::Json {
            json: "data.json".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: LazyValue<String> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LazyValue::Json { json } if json == "data.json"));
    }

    #[test]
    fn test_serde_inline_from_plain_json() {
        // Plain JSON "42" should parse as Inline(42)
        let v: LazyValue<i32> = serde_json::from_str("42").unwrap();
        assert_eq!(v, LazyValue::Inline(42));
    }

    // --- resolve() ---

    #[test]
    fn test_resolve_inline() {
        let v: LazyValue<f64> = LazyValue::Inline(2.72);
        assert!((v.resolve(&ResolveContext::default()).unwrap() - 2.72).abs() < 1e-10);
    }

    #[test]
    fn test_resolve_toml_inline() {
        let v: LazyValue<f64> = LazyValue::Toml {
            toml: "3.14".into(),
        };
        assert!((v.resolve(&ResolveContext::default()).unwrap() - 3.14).abs() < 1e-10);
    }

    #[test]
    fn test_resolve_file() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct V {
            value: i32,
        }
        from_prop_never!(V);
        let path = temp_file("val.toml", "value = 42");
        let v: LazyValue<V> = LazyValue::File {
            file: path.to_string_lossy().into(),
            encoding: Some(Encoding::Toml),
        };
        assert_eq!(v.resolve(&ResolveContext::default()).unwrap().value, 42);
    }

    #[test]
    fn test_resolve_file_json() {
        let path = temp_file("val.json", "\"hello\"");
        let v: LazyValue<String> = LazyValue::File {
            file: path.to_string_lossy().into(),
            encoding: Some(Encoding::Json),
        };
        assert_eq!(v.resolve(&ResolveContext::default()).unwrap(), "hello");
    }

    #[test]
    fn test_resolve_json() {
        let v: LazyValue<String> = LazyValue::Json {
            json: "\"hello\"".into(),
        };
        assert_eq!(v.resolve(&ResolveContext::default()).unwrap(), "hello");
    }

    // --- resolve() with Prop ---

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
        };
        assert!(v.resolve(&ctx).is_err());
    }

    #[test]
    fn test_resolve_prop_type_mismatch() {
        let mut props = HashMap::new();
        props.insert("name".into(), PropertyValue::Number(1.0));

        let v: LazyValue<String> = LazyValue::Prop {
            field: "name".into(),
            encoding: None,
        };
        let ctx = ResolveContext {
            properties: Some(&props),
        };
        assert!(v.resolve(&ctx).is_err());
    }

    #[test]
    fn test_resolve_prop_missing_context() {
        let v: LazyValue<String> = LazyValue::Prop {
            field: "x".into(),
            encoding: None,
        };
        let ctx = ResolveContext::default();
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
