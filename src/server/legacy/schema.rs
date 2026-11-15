//! Version-wide schema inference. XML names are reversible UTF-8 hex tokens;
//! heterogeneous/structured properties remain xs:anyType, without guessed casts.
use super::*;
use crate::PropertyValue;
use std::collections::{BTreeMap, BTreeSet};
pub(super) async fn describe(s: Arc<ServerState>, id: String) -> Result<Response> {
    let key = ResourceKey::new(ResourceKind::Dataset, &id)?;
    let pin = s.catalog.pin(&key)?;
    if pin.records[&key].class.as_deref() != Some("vector") {
        return Err(ServerError::invalid("Vector dataset required"));
    }
    let worker = s.clone();
    let document=s.scheduler.blocking(WorkClass::Http,s.scheduler.context(),move |ctx| {
        let record=&pin.records[&key];
        let path=record.derived_path.as_ref().ok_or_else(||ServerError::new(503,"preparing","Dataset is preparing"))?;
        let store=crate::derived::DerivedVector::open_with_context(path,worker.config.profile.store_limits(),&ctx)?;
        let mut cursor=store.query(crate::readers::VectorQuery::default(),&ctx)?;
        let mut fields:BTreeMap<String,(BTreeSet<&str>,usize,bool)>=BTreeMap::new();let mut count=0usize;let mut holds=vec![];
        while let Some(batch)=cursor.next_batch(worker.config.profile.shard_features,worker.config.profile.max_record_bytes,&ctx)? {
            for feature in &batch.features {
                ctx.checkpoint(1)?;count=count.checked_add(1).ok_or(crate::GeelloError::SizeOverflow)?;
                for (key,value) in &feature.properties {
                    if !fields.contains_key(key) {holds.push(ctx.reserve(key.len().checked_mul(4).and_then(|n|n.checked_add(512)).ok_or(crate::GeelloError::SizeOverflow)?)?);}
                    let field=fields.entry(key.clone()).or_default();field.1+=1;
                    let kind=match value {PropertyValue::Null=>{field.2=true;continue},PropertyValue::Bool(_)=>"xs:boolean",PropertyValue::String(_)=>"xs:string",PropertyValue::I64(_)|PropertyValue::U64(_)=>"xs:integer",PropertyValue::F64(_)=>"xs:double",_=>"xs:anyType"};
                    field.0.insert(kind);
                }
            }
        }
        let _output=ctx.reserve(worker.config.profile.max_registry_bytes)?;
        let mut text=format!(r#"<?xml version="1.0" encoding="UTF-8"?><xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:g="urn:geello:features" targetNamespace="urn:geello:features" elementFormDefault="qualified"><xs:annotation><xs:documentation>Version-wide property schema for GeoJSON output; this XSD does not validate JSON and is not a GML application schema. Original dataset: {}. DataVersion: {}. Names use p followed by UTF-8 hex.</xs:documentation></xs:annotation><xs:element name="{}" type="g:FeatureType"/><xs:complexType name="FeatureType"><xs:sequence><xs:element name="geometry" type="xs:anyType" minOccurs="0" nillable="true"/>"#,xml_escape(&id),xml_escape(&record.data_version),xml_name(&id));
        for (name,(mut kinds,present,nullable)) in fields {
            if kinds.len()==2&&kinds.contains("xs:integer")&&kinds.contains("xs:double") {kinds.remove("xs:integer");}
            let kind=if kinds.len()==1 {*kinds.first().unwrap()}else{"xs:anyType"};
            text.push_str(&format!(r#"<xs:element name="{}" type="{}" minOccurs="{}" nillable="{}"><xs:annotation><xs:documentation>Original property: {}</xs:documentation></xs:annotation></xs:element>"#,xml_name(&name),kind,usize::from(present==count),nullable,xml_escape(&name)));
            if text.len()>worker.config.profile.max_registry_bytes {return Err(ServerError::new(503,"schema-budget","Schema exceeds profile"));}
        }
        text.push_str("</xs:sequence></xs:complexType></xs:schema>");
        let hold=ctx.reserve(text.len())?;
        Ok(crate::Accounted::new(text,vec![hold]))
    }).await?;
    api::bytes_response(&s, document.as_bytes().to_vec(), "text/xml; charset=utf-8")
}
fn xml_name(value: &str) -> String {
    format!(
        "p{}",
        value
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
