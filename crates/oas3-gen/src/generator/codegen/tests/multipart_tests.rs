use quote::ToTokens;

use crate::generator::{
  ast::{
    ContentCategory, FieldNameToken, HandlerBodyInfo, MultipartFieldInfo, MultipartPartKind, OperationBody,
    PartMembers, PartRfc6570, PartStyle, PartType, PartValue, RustPrimitive, TypeRef,
  },
  codegen::multipart::{MultipartDecodeFragment, MultipartFormFragment},
};

fn compact(code: &str) -> String {
  code.split_whitespace().collect()
}

fn field(name: &str, rust_type: TypeRef, kind: MultipartPartKind) -> MultipartFieldInfo {
  MultipartFieldInfo::builder()
    .name(FieldNameToken::from_raw(name))
    .part_name(name)
    .rust_type(rust_type)
    .kind(kind)
    .build()
}

fn value(value: PartValue, content_type: Option<&str>) -> MultipartPartKind {
  MultipartPartKind::Value {
    value,
    content_type: content_type.map(str::to_string),
  }
}

fn file(content_type: &str) -> MultipartPartKind {
  MultipartPartKind::File {
    content_type: content_type.to_string(),
  }
}

fn styled(value: PartValue, style: PartStyle, explode: bool, members: Option<PartMembers>) -> MultipartPartKind {
  MultipartPartKind::Styled {
    value,
    rfc6570: PartRfc6570 { style, explode },
    members,
  }
}

fn part_type(base: impl Into<RustPrimitive>, value: PartValue) -> PartType {
  PartType {
    rust_type: TypeRef::new(base),
    value,
  }
}

fn client_code(fields: Option<Vec<MultipartFieldInfo>>, optional: bool) -> String {
  let body = OperationBody::builder()
    .field_name(FieldNameToken::new("body"))
    .body_type(TypeRef::new("UploadForm"))
    .content_category(ContentCategory::Multipart)
    .optional(optional)
    .maybe_multipart_fields(fields)
    .build();
  compact(&MultipartFormFragment::new(body).into_token_stream().to_string())
}

fn server_code(fields: Option<&[MultipartFieldInfo]>, optional: bool) -> String {
  let body = HandlerBodyInfo::builder()
    .body_type(TypeRef::new("UploadForm"))
    .content_category(ContentCategory::Multipart)
    .optional(optional)
    .maybe_multipart_fields(fields.map(<[MultipartFieldInfo]>::to_vec))
    .build();
  compact(&MultipartDecodeFragment::new(&body).into_token_stream().to_string())
}

#[test]
fn test_client_part_emission() {
  let string = || TypeRef::new(RustPrimitive::String);
  let bytes = || TypeRef::new(RustPrimitive::Bytes);
  let mut serialized_as = field(
    "updated_at",
    TypeRef::new(RustPrimitive::DateTime),
    value(PartValue::Dynamic, None),
  );
  serialized_as.serialize_as = Some("crate::Stamp".to_string());
  let mut deprecated = field("legacy", string(), value(PartValue::String, None));
  deprecated.deprecated = true;

  let cases = [
    (
      field("name", string(), value(PartValue::String, None)),
      "form=form.text(\"name\",body.name);",
    ),
    (
      field("file", bytes().with_option(), file("application/octet-stream")),
      "ifletSome(value)=body.file{form=form.part(\"file\",reqwest::multipart::Part::bytes(value).file_name(\"file\").mime_str(\"application/octet-stream\")?,);}",
    ),
    (
      field("files", bytes().with_vec().with_option(), file("image/png")),
      "forvalueinbody.files.into_iter().flatten(){form=form.part(\"files\",reqwest::multipart::Part::bytes(value).file_name(\"files\").mime_str(\"image/png\")?,);}",
    ),
    (
      field("tags", string().with_vec(), value(PartValue::String, None)),
      "forvalueinbody.tags{form=form.text(\"tags\",value);}",
    ),
    (
      field(
        "count",
        TypeRef::new(RustPrimitive::I64),
        value(PartValue::Scalar, None),
      ),
      "form=form.text(\"count\",body.count.to_string());",
    ),
    (
      field(
        "note",
        string().with_option(),
        value(PartValue::String, Some("text/markdown")),
      ),
      "ifletSome(value)=body.note{form=form.part(\"note\",reqwest::multipart::Part::text(value).mime_str(\"text/markdown\")?);}",
    ),
    (
      field(
        "created_at",
        TypeRef::new(RustPrimitive::DateTime).with_option(),
        value(PartValue::SerdeString, None),
      ),
      "ifletSome(value)=body.created_at&&letserde_json::Value::String(text)=serde_json::to_value(value)?{form=form.text(\"created_at\",text);}",
    ),
    (
      field(
        "address",
        TypeRef::new("Address"),
        value(PartValue::Json, Some("application/xml")),
      ),
      "Part::text(serde_json::to_string(&body.address)?).mime_str(\"application/json\")?",
    ),
    (
      field("note", string(), value(PartValue::Json, Some("application/json"))),
      "Part::text(serde_json::to_string(&body.note)?).mime_str(\"application/json\")?",
    ),
    (
      field(
        "addresses",
        TypeRef::new("Address").with_vec(),
        value(PartValue::Json, Some("application/vnd.example+json")),
      ),
      "Part::text(serde_json::to_string(&value)?).mime_str(\"application/vnd.example+json\")?",
    ),
    (
      serialized_as,
      "serde_json::to_value(serde_with::ser::SerializeAsWrap::<_,crate::Stamp>::new(&body.updated_at))?",
    ),
    (
      field(
        "ids",
        TypeRef::new(RustPrimitive::I64).with_vec(),
        styled(PartValue::Scalar, PartStyle::PipeDelimited, false, None),
      ),
      "letmutitems=vec![];forvalueinbody.ids{items.push(value.to_string());}if!items.is_empty(){form=form.text(\"ids\",items.join(\"|\"));}",
    ),
    (
      field(
        "we{ird}",
        TypeRef::new("Filter").with_option(),
        styled(
          PartValue::Json,
          PartStyle::DeepObject,
          false,
          Some(PartMembers::Named(vec![])),
        ),
      ),
      "ifletSome(value)=body.we_ird&&letserde_json::Value::Object(members)=serde_json::to_value(value)?{for(key,member)inmembers{lettext=matchmember{serde_json::Value::Null=>continue,serde_json::Value::String(text)=>text,other=>other.to_string(),};form=form.text(format!(\"we{{ird}}[{key}]\"),text);}}",
    ),
    (
      field(
        "filter",
        TypeRef::new("Filter"),
        styled(
          PartValue::Json,
          PartStyle::DeepObject,
          false,
          Some(PartMembers::Named(vec![])),
        ),
      ),
      "form=form.text(format!(\"filter[{key}]\"),text);",
    ),
    (
      deprecated,
      "#[allow(deprecated)]{form=form.text(\"legacy\",body.legacy);}",
    ),
    (
      field("labels", TypeRef::new("Labels"), value(PartValue::Dynamic, None)),
      "matchserde_json::to_value(body.labels)?{serde_json::Value::Null=>{}serde_json::Value::String(text)=>{form=form.text(\"labels\",text);}other=>{form=form.part(\"labels\",reqwest::multipart::Part::text(other.to_string()).mime_str(\"application/json\")?,);}}",
    ),
    (
      field(
        "additional_properties",
        TypeRef::new("indexmap::IndexMap<String, Vec<i64>>"),
        MultipartPartKind::Flattened {
          entry: part_type(RustPrimitive::I64, PartValue::Scalar),
          repeated: true,
        },
      ),
      "for(name,values)inbody.additional_properties{forvalueinvalues{form=form.text(name.clone(),value.to_string());}}",
    ),
    (
      field(
        "additional_properties",
        TypeRef::new("indexmap::IndexMap<String, String>"),
        MultipartPartKind::Flattened {
          entry: part_type(RustPrimitive::String, PartValue::String),
          repeated: false,
        },
      ),
      "for(name,value)inbody.additional_properties{form=form.text(name,value);}",
    ),
  ];
  for (field, expected) in cases {
    let name = field.part_name.clone();
    let code = client_code(Some(vec![field]), false);
    assert!(
      code.contains(&compact(expected)),
      "field {name}: expected {expected}\nin {code}"
    );
  }
}

#[test]
fn test_client_body_binding() {
  let name = field(
    "name",
    TypeRef::new(RustPrimitive::String),
    value(PartValue::String, None),
  );
  let note = field(
    "note",
    TypeRef::new(RustPrimitive::String).with_option(),
    value(PartValue::String, None),
  );
  let files = field(
    "files",
    TypeRef::new(RustPrimitive::Bytes).with_vec(),
    file("application/octet-stream"),
  );
  let closing_boundary = "req_builder.header(http::header::CONTENT_TYPE,format!(\"multipart/form-data;boundary={}\",form.boundary())).body(format!(\"--{}--\\r\\n\",form.boundary()))";
  let cases = [
    (
      Some(vec![name.clone(), note.clone()]),
      false,
      vec![
        "letbody=request.body;letmutform=reqwest::multipart::Form::new().percent_encode_noop();",
        "req_builder=req_builder.multipart(form);",
      ],
    ),
    (Some(vec![name]), true, vec!["ifletSome(body)=request.body{letmutform="]),
    (
      Some(vec![note, files]),
      false,
      vec![
        "lethas_parts=body.note.is_some()||!body.files.is_empty();",
        "req_builder=ifhas_parts{req_builder.multipart(form)}else{",
        closing_boundary,
      ],
    ),
    (
      Some(vec![]),
      true,
      vec![
        "ifrequest.body.is_some(){letform=reqwest::multipart::Form::new();req_builder=",
        closing_boundary,
      ],
    ),
    (
      None,
      false,
      vec![
        "letentries=matchserde_json::to_value(body)?{serde_json::Value::Object(entries)=>entries,_=>serde_json::Map::new(),};lethas_parts=entries.values().any(|value|!value.is_null());",
        "for(name,value)inentries{matchvalue{serde_json::Value::Null=>{}serde_json::Value::String(text)=>{form=form.text(name,text);}other=>{form=form.part(name,reqwest::multipart::Part::text(other.to_string()).mime_str(\"application/json\")?,);}}}",
      ],
    ),
  ];
  for (fields, optional, expected) in cases {
    let label = format!("fields={fields:?} optional={optional}");
    let code = client_code(fields, optional);
    for expected in expected {
      assert!(code.contains(expected), "{label}: expected {expected}\nin {code}");
    }
  }
}

#[test]
fn test_server_decode_emission() {
  let bytes = || TypeRef::new(RustPrimitive::Bytes);
  let object = |name: &str, style, explode, members| {
    field(
      name,
      TypeRef::new("Object").with_option(),
      styled(PartValue::Json, style, explode, Some(members)),
    )
  };
  let named_members = || {
    PartMembers::Named(vec![
      ("mood".to_string(), part_type(RustPrimitive::String, PartValue::String)),
      ("score".to_string(), part_type(RustPrimitive::I64, PartValue::Scalar)),
    ])
  };

  let cases = [
    (
      vec![field("avatar", bytes(), file("image/png"))],
      vec![
        "ifname==\"avatar\"{avatar_parts=Some(Vec::from(part.bytes().await?));}",
        "fields.insert(\"avatar\".to_owned(),serde_json::Value::Array(vec![]));",
        "letmutbody:UploadForm=",
        "body.avatar=avatar_parts.ok_or_else(||anyhow::anyhow!(\"missingmultipartpart`{}`\",\"avatar\"))?;",
      ],
    ),
    (
      vec![field(
        "files",
        bytes().with_vec().with_option(),
        file("application/octet-stream"),
      )],
      vec![
        "letfields=serde_json::Map::new();",
        "body.files=(!files_parts.is_empty()).then_some(files_parts);",
      ],
    ),
    (
      vec![field(
        "count",
        TypeRef::new(RustPrimitive::I64),
        value(PartValue::Scalar, None),
      )],
      vec![
        "letmutfields=serde_json::Map::new();",
        "serde_json::from_str(&part.text().await?).map_err(|e|anyhow::anyhow!(\"invalidmultipartpart`{}`:{e}\",\"count\"))?",
        "letbody:UploadForm=",
      ],
    ),
    (
      vec![field(
        "labels",
        TypeRef::new("Labels").with_option(),
        value(PartValue::Dynamic, None),
      )],
      vec![
        "letis_json=part.content_type().is_some_and(|content_type|content_type.contains(\"json\"));",
        "ifserde_json::from_value::<Labels>(serde_json::Value::String(text.clone())).is_ok(){serde_json::Value::String(text)}else{serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))}",
      ],
    ),
    (
      vec![
        field(
          "tags",
          TypeRef::new(RustPrimitive::String).with_vec(),
          value(PartValue::String, None),
        ),
        field(
          "ids",
          TypeRef::new(RustPrimitive::I64).with_vec(),
          styled(PartValue::Scalar, PartStyle::Form, false, None),
        ),
      ],
      vec![
        "#[allow(clippy::similar_names)]",
        "\"tags\"=>{tags_parts.push(serde_json::Value::String(part.text().await?));}",
        "fields.insert(\"tags\".to_owned(),serde_json::Value::Array(tags_parts));",
        "lettext=part.text().await?;if!text.is_empty(){foritemintext.split(','){ids_parts.push(",
      ],
    ),
    (
      vec![
        object("filter", PartStyle::DeepObject, false, named_members()),
        object("range", PartStyle::Form, false, named_members()),
        object("extras", PartStyle::Form, true, named_members()),
        object(
          "options",
          PartStyle::Form,
          true,
          PartMembers::Map(part_type(RustPrimitive::I64, PartValue::Scalar)),
        ),
      ],
      vec![
        "ifname==\"range\"{lettext=part.text().await?;letmuttokens=text.split(',');",
        "letmember=matchkey{\"score\"=>serde_json::from_str(member).map_err(|e|anyhow::anyhow!(\"invalidmultipartpart`{}`:{e}\",\"range\"))?,_=>serde_json::Value::String(member.to_owned()),};",
        "}elseifletSome(key)=name.strip_prefix(\"filter[\").and_then(|key|key.strip_suffix(']')){letmember=matchkey{",
        "filter_parts.insert(key.to_owned(),member);",
        "}elseifmatches!(name.as_str(),\"mood\"|\"score\"){letmember=matchname.as_str(){",
        "extras_parts.insert(name,member);}else{letvalue=serde_json::from_str(&part.text().await?).map_err(|e|anyhow::anyhow!(\"invalidmultipartpart`{name}`:{e}\"))?;options_parts.insert(name,value);}",
      ],
    ),
    (
      vec![field(
        "additional_properties",
        TypeRef::new("indexmap::IndexMap<String, Vec<i64>>"),
        MultipartPartKind::Flattened {
          entry: part_type(RustPrimitive::I64, PartValue::Scalar),
          repeated: true,
        },
      )],
      vec![
        "whileletSome(part)=multipart.next_field().await?{letSome(name)=part.name().map(str::to_owned)else{continue;};letvalue=",
        "ifletserde_json::Value::Array(items)=additional_properties_parts.entry(name).or_insert_with(||serde_json::Value::Array(vec![])){items.push(value);}",
        "fields.extend(additional_properties_parts);",
      ],
    ),
    (
      vec![],
      vec!["letfields=serde_json::Map::new();whilemultipart.next_field().await?.is_some(){}"],
    ),
  ];
  for (fields, expected) in cases {
    let names = fields.iter().map(|field| field.part_name.clone()).collect::<Vec<_>>();
    let code = server_code(Some(&fields), false);
    for expected in expected {
      assert!(
        code.contains(expected),
        "fields {names:?}: expected {expected}\nin {code}"
      );
    }
  }

  let code = server_code(None, true);
  for expected in [
    "letbody=matchmultipart_body(multipart).await{Ok(body)=>body,Err(e)=>return(axum::http::StatusCode::BAD_REQUEST,format!(\"Badrequest:{e}\")).into_response(),};",
    "asyncfnmultipart_body(multipart:Option<axum::extract::Multipart>)->anyhow::Result<Option<UploadForm>>{letSome(mutmultipart)=multipartelse{returnOk(None);};",
    "fields.insert(name,value);",
    "Ok(Some(body))",
  ] {
    assert!(
      code.contains(expected),
      "optional body without a struct: expected {expected}\nin {code}"
    );
  }
}
