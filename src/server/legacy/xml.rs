//! Bounded namespace-aware XML query subset. No DTD, entity resolution or IO.
use super::*;
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
const WFS: &str = "http://www.opengis.net/wfs/2.0";
const FES: &str = "http://www.opengis.net/fes/2.0";
const GML: &str = "http://www.opengis.net/gml/3.2";
struct Node {
    namespace: String,
    name: String,
    attrs: HashMap<String, String>,
    text: String,
    children: Vec<Node>,
}
fn invalid() -> ServerError {
    ServerError::invalid("Unsupported or malformed WFS XML query")
}
fn attrs(node: &Node, allowed: &[&str]) -> Result<()> {
    if node.attrs.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(invalid());
    }
    Ok(())
}
fn leaf(node: &Node, namespace: &str, name: &str) -> Result<()> {
    if node.namespace != namespace
        || node.name != name
        || !node.children.is_empty()
        || !node.attrs.is_empty()
    {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn parse(text: &str) -> Result<(String, HashMap<String, String>)> {
    let mut reader = NsReader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    let mut count = 0usize;
    let mut declared = false;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|_| invalid())?;
        match event {
            Event::Start(element) => {
                if root.is_some() || stack.len() >= 16 || count >= 1024 {
                    return Err(invalid());
                }
                count += 1;
                let ResolveResult::Bound(ns) = ns else {
                    return Err(invalid());
                };
                let mut node = Node {
                    namespace: ns.as_ref().into(),
                    name: element.local_name().as_ref().into(),
                    attrs: HashMap::new(),
                    text: String::new(),
                    children: vec![],
                };
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| invalid())?;
                    let key = attribute.key.as_ref();
                    if key == "xmlns" || key.starts_with("xmlns:") {
                        continue;
                    }
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| invalid())?
                        .into_owned();
                    if node.attrs.insert(key.into(), value).is_some() {
                        return Err(invalid());
                    }
                }
                stack.push(node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(invalid)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Event::Text(value) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(
                        &quick_xml::escape::unescape(value.as_ref()).map_err(|_| invalid())?,
                    );
                } else if !value.as_ref().trim().is_empty() {
                    return Err(invalid());
                }
            }
            Event::GeneralRef(value) => {
                let node = stack.last_mut().ok_or_else(invalid)?;
                if let Some(character) = value.resolve_char_ref().map_err(|_| invalid())? {
                    node.text.push(character);
                } else {
                    node.text.push(match value.as_ref() {
                        "amp" => '&',
                        "lt" => '<',
                        "gt" => '>',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => return Err(invalid()),
                    });
                }
            }
            Event::Decl(value) if stack.is_empty() && root.is_none() && !declared => {
                if value
                    .encoding()
                    .is_some_and(|v| v.map_or(true, |v| !v.eq_ignore_ascii_case("UTF-8")))
                {
                    return Err(invalid());
                }
                declared = true;
            }
            Event::Comment(_) => {}
            Event::Eof if stack.is_empty() => break,
            _ => return Err(invalid()),
        }
    }
    let root = root.ok_or_else(invalid)?;
    if root.namespace != WFS
        || !matches!(
            root.name.as_str(),
            "GetCapabilities" | "DescribeFeatureType" | "GetFeature"
        )
        || !root.text.trim().is_empty()
    {
        return Err(invalid());
    }
    attrs(
        &root,
        &[
            "service",
            "version",
            "typeNames",
            "typename",
            "outputFormat",
            "count",
            "startIndex",
            "bbox",
            "resultType",
        ],
    )?;
    let mut result = root.attrs.clone();
    if root.name != "GetFeature" {
        if !root.children.is_empty() {
            return Err(invalid());
        }
    } else {
        if root.children.len() > 1 {
            return Err(invalid());
        }
        if let Some(query) = root.children.first() {
            if query.namespace != WFS || query.name != "Query" || !query.text.trim().is_empty() {
                return Err(invalid());
            }
            attrs(query, &["typeNames", "srsName"])?;
            for (key, value) in &query.attrs {
                if key == "srsName" {
                    if !matches!(
                        value.as_str(),
                        "OGC:CRS84"
                            | "urn:ogc:def:crs:OGC::CRS84"
                            | "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
                    ) {
                        return Err(invalid());
                    }
                } else if result.insert(key.clone(), value.clone()).is_some() {
                    return Err(invalid());
                }
            }
            if query.children.len() > 1 {
                return Err(invalid());
            }
            if let Some(filter) = query.children.first() {
                if filter.namespace != FES
                    || filter.name != "Filter"
                    || !filter.attrs.is_empty()
                    || !filter.text.trim().is_empty()
                    || filter.children.is_empty()
                {
                    return Err(invalid());
                }
                if filter
                    .children
                    .iter()
                    .all(|c| c.namespace == FES && c.name == "ResourceId")
                {
                    let mut ids = vec![];
                    for node in &filter.children {
                        attrs(node, &["rid"])?;
                        if !node.children.is_empty() || !node.text.trim().is_empty() {
                            return Err(invalid());
                        }
                        let id = node.attrs.get("rid").ok_or_else(invalid)?;
                        features::decode_feature_id(id)?;
                        ids.push(id.clone());
                    }
                    result.insert("ids".into(), ids.join(","));
                } else {
                    if filter.children.len() != 1 {
                        return Err(invalid());
                    }
                    condition(&filter.children[0], &mut result)?;
                }
            }
        }
    }
    Ok((root.name, result))
}
fn condition(node: &Node, out: &mut HashMap<String, String>) -> Result<()> {
    if node.namespace != FES || !node.attrs.is_empty() || !node.text.trim().is_empty() {
        return Err(invalid());
    }
    match node.name.as_str() {
        "And" => {
            if node.children.len() < 2 {
                return Err(invalid());
            }
            for child in &node.children {
                condition(child, out)?;
            }
        }
        "PropertyIsEqualTo" => {
            if node.children.len() != 2 {
                return Err(invalid());
            }
            let field = &node.children[0];
            leaf(field, FES, "ValueReference")?;
            let value = &node.children[1];
            leaf(value, FES, "Literal")?;
            let field = field.text.trim();
            if field.is_empty() {
                return Err(invalid());
            }
            if out
                .insert(format!("eq:{field}"), value.text.clone())
                .is_some()
            {
                return Err(invalid());
            }
        }
        "BBOX" => {
            if node.children.len() != 2 {
                return Err(invalid());
            }
            leaf(&node.children[0], FES, "ValueReference")?;
            if node.children[0].text.trim() != "geometry" {
                return Err(invalid());
            }
            let envelope = &node.children[1];
            if envelope.namespace != GML
                || envelope.name != "Envelope"
                || envelope.children.len() != 2
                || !envelope.text.trim().is_empty()
            {
                return Err(invalid());
            }
            attrs(envelope, &["srsName"])?;
            if !matches!(
                envelope.attrs.get("srsName").map(String::as_str),
                Some(
                    "OGC:CRS84"
                        | "urn:ogc:def:crs:OGC::CRS84"
                        | "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
                )
            ) {
                return Err(invalid());
            }
            leaf(&envelope.children[0], GML, "lowerCorner")?;
            leaf(&envelope.children[1], GML, "upperCorner")?;
            let lower = envelope.children[0]
                .text
                .split_whitespace()
                .collect::<Vec<_>>();
            let upper = envelope.children[1]
                .text
                .split_whitespace()
                .collect::<Vec<_>>();
            if lower.len() != 2 || upper.len() != 2 {
                return Err(invalid());
            }
            let bbox = format!("{},{},{},{}", lower[0], lower[1], upper[0], upper[1]);
            parse_bbox(&bbox)?;
            if out.insert("bbox".into(), bbox).is_some() {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
