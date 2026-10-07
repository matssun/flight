// SPDX-License-Identifier: MIT

//! proto/flight.proto is the normative wire schema. This test extracts the schema from the
//! hand-written prost types and from the .proto and requires them to be identical: message
//! and field names, numbers, types, repeated-ness, oneofs and enum values.

use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn snake(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

/// "field Container.name = N : type" style lines, plus enum values and oneofs.
type Schema = BTreeSet<String>;

fn strip_option_vec(ty: &str) -> String {
    let ty = ty.trim();
    for wrapper in ["Option<", "Vec<"] {
        if let Some(inner) = ty.strip_prefix(wrapper).and_then(|t| t.strip_suffix('>')) {
            return inner.trim().to_owned();
        }
    }
    ty.to_owned()
}

/// What a prost attribute says about a field's type.
fn attr_type(attr: &str, rust_ty: &str) -> (String, bool) {
    let repeated = attr.contains("repeated");
    let kind = attr.split(',').next().unwrap_or("").trim();
    let ty = if kind.starts_with("enumeration") {
        re(r#"enumeration\s*=\s*"(\w+)""#)
            .captures(attr)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default()
    } else if kind.starts_with("bytes") {
        "bytes".to_owned()
    } else if kind == "message" {
        strip_option_vec(rust_ty)
    } else {
        kind.to_owned()
    };
    (ty, repeated)
}

fn rust_schema() -> Schema {
    let mut schema = Schema::new();
    let header = re(r"pub (struct|enum) (\w+)");
    let module = re(r"pub mod (\w+)");
    let field =
        re(r"#\[prost\(([^\]]*)\)\]\s*(?:///[^\n]*\n\s*)*(?://[^\n]*\n\s*)*pub (\w+): ([^,\n]+),");
    let variant = re(r"#\[prost\(([^\]]*)\)\]\s*(\w+)\((\w+)\),");
    let wire_enum = re(r"wire_enum!\s*\{\s*(?:///[^\n]*\n\s*)*(\w+)\s*\{([^}]*)\}");
    let oneof_attr = re(r#"oneof\s*=\s*"(\w+)::(\w+)""#);

    let mut oneof_parent: Vec<(String, String, String)> = Vec::new(); // (mod, enum, parent)
    let mut files: Vec<String> = Vec::new();
    for entry in fs::read_dir(root().join("src")).expect("src dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            files.push(fs::read_to_string(path).expect("read source"));
        }
    }

    for text in &files {
        for c in field.captures_iter(text) {
            let pos = c.get(0).map_or(0, |m| m.start());
            let container = header
                .captures_iter(&text[..pos])
                .last()
                .map(|h| h[2].to_owned())
                .unwrap_or_default();
            let (attr, name, ty) = (&c[1], &c[2], &c[3]);
            if let Some(o) = oneof_attr.captures(attr) {
                schema.insert(format!("oneof {container}.{name}"));
                oneof_parent.push((o[1].to_owned(), o[2].to_owned(), container.clone()));
                continue;
            }
            let tag = re(r#"tag\s*=\s*"(\d+)""#)
                .captures(attr)
                .map(|t| t[1].to_owned())
                .unwrap_or_default();
            let (ty, repeated) = attr_type(attr, ty);
            let rep = if repeated { "repeated " } else { "" };
            schema.insert(format!("field {container}.{name} = {tag} : {rep}{ty}"));
        }
        for v in variant.captures_iter(text) {
            let pos = v.get(0).map_or(0, |m| m.start());
            let module_name = module
                .captures_iter(&text[..pos])
                .last()
                .map(|m| m[1].to_owned())
                .unwrap_or_default();
            let enum_name = header
                .captures_iter(&text[..pos])
                .last()
                .map(|h| h[2].to_owned())
                .unwrap_or_default();
            let parent = oneof_parent
                .iter()
                .find(|(m, e, _)| *m == module_name && *e == enum_name)
                .map(|(_, _, p)| p.clone());
            // The struct holding the oneof field may be defined after the enum in the file.
            let parent = parent.or_else(|| {
                oneof_attr
                    .captures_iter(text)
                    .find(|o| o[1] == module_name && o[2] == enum_name)
                    .and_then(|o| {
                        let pos = o.get(0).map_or(0, |m| m.start());
                        header
                            .captures_iter(&text[..pos])
                            .last()
                            .map(|h| h[2].to_owned())
                    })
            });
            let tag = re(r#"tag\s*=\s*"(\d+)""#)
                .captures(&v[1])
                .map(|t| t[1].to_owned())
                .unwrap_or_default();
            let parent = parent.unwrap_or_else(|| format!("?{module_name}::{enum_name}"));
            schema.insert(format!(
                "field {parent}.{} = {tag} : {}",
                snake(&v[2]),
                &v[3]
            ));
        }
        for e in wire_enum.captures_iter(text) {
            let prefix = snake(&e[1]).to_uppercase();
            schema.insert(format!("enum {}", &e[1]));
            schema.insert(format!("value {} {prefix}_UNSPECIFIED = 0", &e[1]));
            for item in e[2].split(',') {
                if let Some((name, n)) = item.split_once('=') {
                    schema.insert(format!(
                        "value {} {prefix}_{} = {}",
                        &e[1],
                        snake(name.trim()).to_uppercase(),
                        n.trim()
                    ));
                }
            }
        }
    }
    schema
}

fn proto_schema() -> Schema {
    let text = fs::read_to_string(root().join("proto/flight.proto")).expect("proto file");
    let mut schema = Schema::new();
    let mut stack: Vec<(String, String)> = Vec::new(); // (kind, name)
    let open = re(r"^(message|enum|oneof)\s+(\w+)\s*\{(\s*\})?$");
    let field = re(r"^(repeated\s+)?([\w.]+)\s+(\w+)\s*=\s*(\d+);$");
    let value = re(r"^(\w+)\s*=\s*(\d+);$");
    for raw in text.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with("syntax") || line.starts_with("package") {
            continue;
        }
        if let Some(c) = open.captures(line) {
            let (kind, name) = (c[1].to_owned(), c[2].to_owned());
            if kind == "enum" {
                schema.insert(format!("enum {name}"));
            }
            if kind == "oneof" {
                let parent = nearest_message(&stack);
                schema.insert(format!("oneof {parent}.{name}"));
            }
            if c.get(3).is_none() {
                stack.push((kind, name));
            }
            continue;
        }
        if line == "}" {
            stack.pop();
            continue;
        }
        match stack.last() {
            Some((kind, name)) if kind == "enum" => {
                if let Some(c) = value.captures(line) {
                    schema.insert(format!("value {name} {} = {}", &c[1], &c[2]));
                }
            }
            Some(_) => {
                if let Some(c) = field.captures(line) {
                    let rep = c.get(1).map_or("", |_| "repeated ");
                    schema.insert(format!(
                        "field {}.{} = {} : {rep}{}",
                        nearest_message(&stack),
                        &c[3],
                        &c[4],
                        &c[2]
                    ));
                }
            }
            None => {}
        }
    }
    schema
}

fn nearest_message(stack: &[(String, String)]) -> String {
    stack
        .iter()
        .rev()
        .find(|(k, _)| k == "message")
        .map(|(_, n)| n.clone())
        .unwrap_or_default()
}

#[test]
fn the_rust_types_and_the_proto_describe_the_same_schema() {
    let rust = rust_schema();
    let proto = proto_schema();
    let only_rust: Vec<_> = rust.difference(&proto).collect();
    let only_proto: Vec<_> = proto.difference(&rust).collect();
    assert!(
        only_rust.is_empty() && only_proto.is_empty(),
        "schema drift\n  only in Rust:\n    {}\n  only in .proto:\n    {}",
        only_rust
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n    "),
        only_proto
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n    "),
    );
}

#[test]
fn the_extraction_is_not_vacuous() {
    let rust = rust_schema();
    assert!(rust.len() > 150, "only {} schema items found", rust.len());
    for expected in [
        "field PaneState.state = 3 : StateCode",
        "field Delta.pane_removed = 4 : PaneRefMsg",
        "field NodeHello.capabilities = 4 : repeated string",
        "field Snapshot.incarnation = 1 : bytes",
        "oneof NodeFrame.body",
        "value StateCode STATE_CODE_PERMIT = 1",
    ] {
        assert!(rust.contains(expected), "missing {expected}");
    }
}
