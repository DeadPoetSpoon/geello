//! Typed property predicates shared by readers and style rules.
use crate::{GeelloError, Properties, PropertyValue};
use serde::Deserialize;

/// SQL-like unknown is retained until the final match decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}
impl Truth {
    pub fn matches(self) -> bool {
        self == Self::True
    }
    fn negate(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

/// An explicit JSON Pointer to a property. The root object is not a scalar field.
pub fn property<'a>(
    properties: &'a Properties,
    pointer: &str,
) -> Result<Option<&'a PropertyValue>, GeelloError> {
    let tokens = pointer_tokens(pointer)?;
    let mut tokens = tokens.iter();
    let Some(first) = tokens.next() else {
        return Ok(None);
    };
    let mut current = properties.get(first);
    for token in tokens {
        current = match current {
            Some(PropertyValue::Object(map)) => map.get(token),
            Some(PropertyValue::Array(values))
                if !token.is_empty()
                    && token.bytes().all(|b| b.is_ascii_digit())
                    && (token == "0" || !token.starts_with('0')) =>
            {
                token.parse::<usize>().ok().and_then(|n| values.get(n))
            }
            _ => None,
        };
    }
    Ok(current)
}
pub fn pointer_tokens(pointer: &str) -> Result<Vec<String>, GeelloError> {
    if pointer.is_empty() {
        return Err(GeelloError::InvalidConfiguration(
            "scalar property pointer cannot select the root object".into(),
        ));
    }
    if !pointer.starts_with('/') {
        return Err(GeelloError::InvalidConfiguration(
            "JSON Pointer must start with /".into(),
        ));
    }
    pointer[1..]
        .split('/')
        .map(|part| {
            let mut out = String::new();
            let mut chars = part.chars();
            while let Some(c) = chars.next() {
                if c == '~' {
                    out.push(match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => {
                            return Err(GeelloError::InvalidConfiguration(
                                "invalid JSON Pointer escape".into(),
                            ));
                        }
                    });
                } else {
                    out.push(c)
                }
            }
            Ok(out)
        })
        .collect()
}

/// Tagged predicates have no implicit scalar coercion. Unknown never matches.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Filter {
    Eq {
        field: String,
        value: PropertyValue,
    },
    Neq {
        field: String,
        value: PropertyValue,
    },
    Lt {
        field: String,
        value: PropertyValue,
    },
    Lte {
        field: String,
        value: PropertyValue,
    },
    Gt {
        field: String,
        value: PropertyValue,
    },
    Gte {
        field: String,
        value: PropertyValue,
    },
    In {
        field: String,
        values: Vec<PropertyValue>,
    },
    Exists {
        field: String,
    },
    IsNull {
        field: String,
    },
    And {
        args: Vec<Filter>,
    },
    Or {
        args: Vec<Filter>,
    },
    Not {
        arg: Box<Filter>,
    },
}
impl Filter {
    /// Validate every pointer before reading records, including short-circuited arms.
    pub fn validate(&self) -> Result<(), GeelloError> {
        use Filter::*;
        match self {
            And { args } | Or { args } => {
                for arg in args {
                    arg.validate()?;
                }
            }
            Not { arg } => arg.validate()?,
            Eq { field, .. }
            | Neq { field, .. }
            | Lt { field, .. }
            | Lte { field, .. }
            | Gt { field, .. }
            | Gte { field, .. }
            | In { field, .. }
            | Exists { field }
            | IsNull { field } => {
                pointer_tokens(field)?;
            }
        }
        Ok(())
    }
    pub fn evaluate(&self, properties: &Properties) -> Result<Truth, GeelloError> {
        use Filter::*;
        use Truth::*;
        Ok(match self {
            And { args } => {
                let mut unknown = false;
                for arg in args {
                    match arg.evaluate(properties)? {
                        False => return Ok(False),
                        Unknown => unknown = true,
                        _ => {}
                    }
                }
                if unknown { Unknown } else { True }
            }
            Or { args } => {
                let mut unknown = false;
                for arg in args {
                    match arg.evaluate(properties)? {
                        True => return Ok(True),
                        Unknown => unknown = true,
                        _ => {}
                    }
                }
                if unknown { Unknown } else { False }
            }
            Not { arg } => arg.evaluate(properties)?.negate(),
            Exists { field } => {
                if property(properties, field)?.is_some() {
                    True
                } else {
                    False
                }
            }
            IsNull { field } => match property(properties, field)? {
                Some(PropertyValue::Null) => True,
                Some(_) => False,
                None => Unknown,
            },
            In { field, values } => {
                let Some(v) = property(properties, field)? else {
                    return Ok(Unknown);
                };
                let mut unknown = false;
                for rhs in values {
                    match compare(v, rhs) {
                        Some(std::cmp::Ordering::Equal) => return Ok(True),
                        None => unknown = true,
                        _ => {}
                    }
                }
                if unknown { Unknown } else { False }
            }
            Eq { field, value }
            | Neq { field, value }
            | Lt { field, value }
            | Lte { field, value }
            | Gt { field, value }
            | Gte { field, value } => {
                let Some(v) = property(properties, field)? else {
                    return Ok(Unknown);
                };
                let Some(order) = compare(v, value) else {
                    return Ok(Unknown);
                };
                let yes = match self {
                    Eq { .. } => order.is_eq(),
                    Neq { .. } => !order.is_eq(),
                    Lt { .. } => order.is_lt(),
                    Lte { .. } => !order.is_gt(),
                    Gt { .. } => order.is_gt(),
                    Gte { .. } => !order.is_lt(),
                    _ => unreachable!(),
                };
                if yes { True } else { False }
            }
        })
    }
}
/// Compare integers without passing their full ranges through f64.
fn compare(a: &PropertyValue, b: &PropertyValue) -> Option<std::cmp::Ordering> {
    use PropertyValue::*;
    match (a, b) {
        (String(a), String(b)) => Some(a.cmp(b)),
        (Bool(a), Bool(b)) => Some(a.cmp(b)),
        (I64(a), I64(b)) => Some(a.cmp(b)),
        (U64(a), U64(b)) => Some(a.cmp(b)),
        (I64(a), U64(b)) => Some(if *a < 0 {
            std::cmp::Ordering::Less
        } else {
            (*a as u64).cmp(b)
        }),
        (U64(a), I64(b)) => Some(if *b < 0 {
            std::cmp::Ordering::Greater
        } else {
            a.cmp(&(*b as u64))
        }),
        (F64(a), F64(b)) => a.partial_cmp(b),
        // Cross-kind floating comparisons are deliberately unknown if an integer
        // cannot be represented exactly. No large identity is rounded to match.
        (F64(a), I64(b)) if b.unsigned_abs() <= 1 << 53 => a.partial_cmp(&(*b as f64)),
        (I64(a), F64(b)) if a.unsigned_abs() <= 1 << 53 => (*a as f64).partial_cmp(b),
        (F64(a), U64(b)) if *b <= 1 << 53 => a.partial_cmp(&(*b as f64)),
        (U64(a), F64(b)) if *a <= 1 << 53 => (*a as f64).partial_cmp(b),
        _ => None,
    }
}
