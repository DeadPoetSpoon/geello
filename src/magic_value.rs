use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum StrEncoding {
    #[default]
    Ron,
    #[cfg(feature = "from_json")]
    Json,
}

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
pub enum MagicValueKind {
    #[default]
    Fixed,
    Prop(String, StrEncoding),
    Ron(String),
    #[cfg(feature = "from_json")]
    Json(String),
    #[cfg(feature = "from_http")]
    Http(String, StrEncoding),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagicValue<T> {
    #[serde(flatten)]
    #[serde(default)]
    inner: T,
    #[serde(default)]
    kind: MagicValueKind,
    #[serde(default)]
    need_scale: bool,
}

impl<T> MagicValue<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            kind: MagicValueKind::Fixed,
            need_scale: false,
        }
    }
    pub fn unwrap(self) -> T {
        self.inner
    }
    pub fn as_ref(&self) -> &T {
        &self.inner
    }
    pub fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> From<T> for MagicValue<T> {
    fn from(value: T) -> Self {
        MagicValue::new(value)
    }
}

impl MagicValue<PropValue> {
    pub fn to_f32(self) -> Result<f32, String> {
        self.inner.try_into()
    }
    pub fn to_f64(self) -> Result<f64, String> {
        self.inner.try_into()
    }
    pub fn to_string(self) -> Result<String, String> {
        self.inner.try_into()
    }
    pub fn wrap<D: Into<PropValue>>(value: D) -> Self {
        Self {
            inner: Into::<PropValue>::into(value),
            kind: MagicValueKind::Fixed,
            need_scale: false,
        }
    }
}

impl<T> MagicValue<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    pub fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        match &self.kind {
            MagicValueKind::Prop(name, encoding) => {
                if let Some(value) = props.get(name) {
                    let mut v: MagicValue<T> = match encoding {
                        StrEncoding::Ron => ron::from_str(&value.to_string()).map_err(|e| {
                            format!("Deserializing value from Prop:{} error: {}", name, e)
                        })?,
                        #[cfg(feature = "from_json")]
                        StrEncoding::Json => {
                            serde_json::from_str(&value.to_string()).map_err(|e| {
                                format!("Deserializing value from Prop:{} error: {}", name, e)
                            })?
                        }
                    };
                    v.fetch()?;
                    self.inner = v.unwrap();
                    Ok(())
                } else {
                    return Err(format!("No {} found in props", name));
                }
            }
            _ => Ok(()),
        }
    }
    pub fn fetch(&mut self) -> Result<(), String> {
        let inner = match &self.kind {
            MagicValueKind::Ron(path) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Read value from File:{} error: {}", path, e))?;
                let mut value: MagicValue<T> = ron::from_str(&content)
                    .map_err(|e| format!("Deserializing value from File:{} error: {}", path, e))?;
                value.fetch()?;
                Some(value.unwrap())
            }
            #[cfg(feature = "from_json")]
            MagicValueKind::Json(path) => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Read value from File:{} error: {}", path, e))?;
                let mut value: MagicValue<T> = serde_json::from_str(&content)
                    .map_err(|e| format!("Deserializing value from File:{} error: {}", path, e))?;
                value.fetch()?;
                Some(value.unwrap())
            }
            #[cfg(feature = "from_http")]
            MagicValueKind::Http(url, encoding) => {
                let res = reqwest::blocking::get(url)
                    .map_err(|e| format!("Get value from Url:{} error: {}", url, e))?;
                let text = res
                    .text()
                    .map_err(|e| format!("Get text from Url:{} error: {}", url, e))?;
                let mut value: MagicValue<T> = match encoding {
                    StrEncoding::Ron => ron::from_str(&text).map_err(|e| {
                        format!("Deserializing value from Url:{} error: {}", url, e)
                    })?,
                    #[cfg(feature = "from_json")]
                    StrEncoding::Json => serde_json::from_str(&text).map_err(|e| {
                        format!("Deserializing value from Url:{} error: {}", url, e)
                    })?,
                };
                value.fetch()?;
                Some(value.unwrap())
            }
            _ => None,
        };
        if inner.is_some() {
            self.inner = inner.unwrap();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PropValue {
    None,
    String(String),
    Float64(f64),
    Float32(f32),
    Int32(i32),
    Int64(i64),
    Boolean(bool),
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

impl TryInto<String> for PropValue {
    type Error = String;
    fn try_into(self) -> Result<String, Self::Error> {
        match self {
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

impl TryInto<f32> for PropValue {
    type Error = String;
    fn try_into(self) -> Result<f32, Self::Error> {
        match self {
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
            }
            PropValue::None => Err("Cannot convert None to f32".to_string()),
        }
    }
}

impl TryInto<f64> for PropValue {
    type Error = String;
    fn try_into(self) -> Result<f64, Self::Error> {
        match self {
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
            }
            PropValue::None => Err("Cannot convert None to f64".to_string()),
        }
    }
}
