use crate::model::{Direction, Field, RustType, SignalClass};

/// Parse signals/mod.rs and return all signal classes in declaration order.
pub fn parse_signals(src: &str) -> Result<Vec<SignalClass>, Box<dyn std::error::Error>> {
    let file = syn::parse_file(src)?;
    let mut signals = Vec::new();

    for item in &file.items {
        if let syn::Item::Struct(s) = item {
            // Skip structs with no pub fields (likely unit structs that are fine but
            // skip if they have impl blocks implying they aren't pure signals).
            // We process ALL pub structs in the file.
            let derives = collect_derives(s);
            let has_serialize = derives.iter().any(|d| d == "Serialize");
            let has_deserialize = derives.iter().any(|d| d == "Deserialize");

            if !has_serialize && !has_deserialize {
                // Not a signal type (e.g., a helper struct without serde derives)
                continue;
            }

            let name = s.ident.to_string();
            let direction = classify_direction(&name, has_serialize, has_deserialize);
            let fields = parse_fields(s)?;

            signals.push(SignalClass {
                name,
                direction,
                has_serialize,
                has_deserialize,
                fields,
            });
        }
    }

    Ok(signals)
}

fn classify_direction(name: &str, has_serialize: bool, has_deserialize: bool) -> Direction {
    // Primary classification by name suffix + derive combination:
    // - "Response" or "Update" suffix with Serialize → Rust2Dart
    // - "Request" suffix with Deserialize → Dart2Rust
    // - Everything else → Shared (sub-struct)
    if has_serialize && (name.ends_with("Response") || name.ends_with("Update")) {
        Direction::Rust2Dart
    } else if has_deserialize && name.ends_with("Request") {
        Direction::Dart2Rust
    } else {
        Direction::Shared
    }
}

fn collect_derives(item: &syn::ItemStruct) -> Vec<String> {
    item.attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("derive") {
                return None;
            }
            let meta_list = attr.meta.require_list().ok()?;
            let paths = meta_list
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                )
                .ok()?;
            Some(
                paths
                    .iter()
                    .filter_map(|p| p.segments.last().map(|s| s.ident.to_string()))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

fn parse_fields(
    item: &syn::ItemStruct,
) -> Result<Vec<Field>, Box<dyn std::error::Error>> {
    let mut fields = Vec::new();
    if let syn::Fields::Named(named) = &item.fields {
        for field in &named.named {
            let name = field
                .ident
                .as_ref()
                .ok_or("unnamed field")?
                .to_string();
            let ty = parse_type(&field.ty)?;
            let has_serde_default = check_serde_default(field);
            fields.push(Field {
                name,
                ty,
                has_serde_default,
            });
        }
    }
    Ok(fields)
}

fn parse_type(ty: &syn::Type) -> Result<RustType, Box<dyn std::error::Error>> {
    match ty {
        syn::Type::Path(p) => {
            let segment = p
                .path
                .segments
                .last()
                .ok_or("empty path")?;
            let ident = segment.ident.to_string();
            match ident.as_str() {
                "String" => Ok(RustType::Str),
                "u8" => Ok(RustType::U8),
                "u32" => Ok(RustType::U32),
                "u64" => Ok(RustType::U64),
                "bool" => Ok(RustType::Bool),
                "Vec" | "Option" => {
                    let args = match &segment.arguments {
                        syn::PathArguments::AngleBracketed(a) => &a.args,
                        _ => return Err("expected angle-bracketed args".into()),
                    };
                    let inner = args.first().ok_or("no type arg")?;
                    if let syn::GenericArgument::Type(inner_ty) = inner {
                        let inner_parsed = parse_type(inner_ty)?;
                        if ident == "Vec" {
                            Ok(RustType::Vec(Box::new(inner_parsed)))
                        } else {
                            Ok(RustType::Option(Box::new(inner_parsed)))
                        }
                    } else {
                        Err("unexpected generic arg".into())
                    }
                }
                other => Ok(RustType::Named(other.to_string())),
            }
        }
        syn::Type::Tuple(t) => {
            let elems: Result<Vec<_>, _> = t.elems.iter().map(parse_type).collect();
            Ok(RustType::Tuple(elems?))
        }
        _ => Err("unsupported type variant".into()),
    }
}

fn check_serde_default(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        if !attr.path().is_ident("serde") {
            return false;
        }
        let Ok(list) = attr.meta.require_list() else {
            return false;
        };
        // Parse each meta item and look for "default"
        let nested = list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .unwrap_or_default();
        nested.iter().any(|m| m.path().is_ident("default"))
    })
}
