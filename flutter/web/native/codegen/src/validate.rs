//! Checks on `signals/mod.rs` that the two compilers between it and the browser cannot make.
//!
//! The signal surface is one contract split across two languages. Each half compiles on its
//! own, so a change that breaks only the agreement between them produces no error anywhere —
//! it produces a `TypeError` inside a Web Worker, once, when the user hits that feature. This
//! is where the halves are held next to each other, so it is where such a change has to be
//! caught.

use std::collections::{HashMap, HashSet};

use crate::dart_gen::from_json_tolerates_missing;
use crate::model::{Direction, RustType, SignalClass};

/// Reject serde attributes that let Rust omit a key the generated Dart insists on.
///
/// `fromJson` casts a required field straight out of the map — `(json['outputs'] as List)` —
/// so a key Rust decided not to serialise arrives as `null` and throws. That strictness is
/// right: a response missing a field is a bug, and defaulting it silently would hide it. But
/// it holds only while nothing on the Rust side skips a field, and today nothing does. One
/// `#[serde(skip_serializing_if = "Vec::is_empty")]` on a response — the ordinary, idiomatic
/// thing to write — and an empty list becomes a crash in the worker.
///
/// So the attribute is refused here, at generation time, where the tier already runs.
pub fn check_serialization_skips(signals: &[SignalClass]) -> Result<(), String> {
    let decoded = dart_decoded_structs(signals);
    let mut problems = Vec::new();

    for sig in signals.iter().filter(|s| decoded.contains(&s.name)) {
        for field in &sig.fields {
            let Some(attr) = &field.skips_serializing else {
                continue;
            };
            if from_json_tolerates_missing(field) {
                continue;
            }
            problems.push(format!(
                "  {}.{}: #[serde({})] on a field Dart reads as `{}`.\n    \
                 Dart's fromJson casts that key out of the map, so the signal throws in the \
                 worker whenever Rust omits it.\n    \
                 Make the field Option<T> (Dart then reads it as nullable), or drop the \
                 attribute and always send the key.",
                sig.name,
                field.name,
                attr,
                crate::dart_gen::dart_type(&field.ty),
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "signals/mod.rs lets Rust omit fields the generated Dart requires:\n{}",
        problems.join("\n")
    ))
}

/// Names of the structs Dart deserialises: the Rust→Dart signals, plus every struct
/// reachable from one through a field type.
///
/// A `Shared` sub-struct is decoded by Dart only when a response carries it. One nested
/// solely in a `Request` travels the other way, is serialised by Dart and decoded by Rust,
/// and a skip on it is harmless — so reachability is walked rather than assumed.
fn dart_decoded_structs(signals: &[SignalClass]) -> HashSet<String> {
    let by_name: HashMap<&str, &SignalClass> =
        signals.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut seen = HashSet::new();
    let mut queue: Vec<&str> = signals
        .iter()
        .filter(|s| s.direction == Direction::Rust2Dart)
        .map(|s| s.name.as_str())
        .collect();

    while let Some(name) = queue.pop() {
        if !seen.insert(name.to_string()) {
            continue;
        }
        let Some(sig) = by_name.get(name) else {
            continue;
        };
        for field in &sig.fields {
            collect_named(&field.ty, &mut queue);
        }
    }
    seen
}

fn collect_named<'a>(ty: &'a RustType, out: &mut Vec<&'a str>) {
    match ty {
        RustType::Named(n) => out.push(n.as_str()),
        RustType::Vec(inner) | RustType::Option(inner) => collect_named(inner, out),
        RustType::Tuple(elems) => elems.iter().for_each(|e| collect_named(e, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_signals;

    fn check(src: &str) -> Result<(), String> {
        check_serialization_skips(&parse_signals(src).expect("parse"))
    }

    #[test]
    fn skipping_a_required_vec_on_a_response_is_rejected() {
        let err = check(
            r#"
            #[derive(Serialize)]
            pub struct ListOutputsResponse {
                #[serde(skip_serializing_if = "Vec::is_empty")]
                pub outputs: Vec<String>,
            }
            "#,
        )
        .expect_err("a skipped Vec must not pass");
        assert!(err.contains("ListOutputsResponse.outputs"), "{err}");
        assert!(err.contains("skip_serializing_if"), "{err}");
        assert!(err.contains("List<String>"), "{err}");
    }

    #[test]
    fn skipping_a_required_scalar_on_a_response_is_rejected() {
        let err = check(
            r#"
            #[derive(Serialize)]
            pub struct BalanceUpdate {
                #[serde(skip)]
                pub address: String,
            }
            "#,
        )
        .expect_err("a skipped String must not pass");
        assert!(err.contains("BalanceUpdate.address"), "{err}");
    }

    /// `Option` is the escape hatch the error message recommends, so it has to work.
    #[test]
    fn skipping_an_option_is_allowed() {
        check(
            r#"
            #[derive(Serialize)]
            pub struct ScanResponse {
                #[serde(skip_serializing_if = "Option::is_none")]
                pub error: Option<String>,
            }
            "#,
        )
        .expect("Dart reads an Option as nullable, so an absent key is fine");
    }

    #[test]
    fn skipping_a_defaulted_bool_is_allowed() {
        check(
            r#"
            #[derive(Serialize)]
            pub struct ScanResponse {
                #[serde(default, skip_serializing_if = "std::ops::Not::not")]
                pub done: bool,
            }
            "#,
        )
        .expect("the generated fromJson ends that one in `?? false`");
    }

    /// Dart writes requests and never reads them, so Rust's serialisation of one is moot.
    #[test]
    fn skipping_a_field_on_a_request_is_allowed() {
        check(
            r#"
            #[derive(Deserialize)]
            pub struct StartScanRequest {
                #[serde(skip_serializing_if = "Vec::is_empty")]
                pub wallets: Vec<String>,
            }
            "#,
        )
        .expect("a Dart2Rust request is not decoded by Dart");
    }

    #[test]
    fn a_shared_struct_reached_by_a_response_is_checked() {
        let err = check(
            r#"
            #[derive(Serialize, Deserialize)]
            pub struct OwnedOutput {
                #[serde(skip_serializing_if = "Vec::is_empty")]
                pub key_image: Vec<u8>,
            }
            #[derive(Serialize)]
            pub struct OutputsResponse {
                pub outputs: Vec<OwnedOutput>,
            }
            "#,
        )
        .expect_err("a sub-struct of a response is decoded by Dart too");
        assert!(err.contains("OwnedOutput.key_image"), "{err}");
    }

    /// The same struct nested only in a request stays Dart's to write, not to read.
    #[test]
    fn a_shared_struct_reached_only_by_a_request_is_not_checked() {
        check(
            r#"
            #[derive(Serialize, Deserialize)]
            pub struct Recipient {
                #[serde(skip_serializing_if = "Vec::is_empty")]
                pub tag: Vec<u8>,
            }
            #[derive(Deserialize)]
            pub struct SendRequest {
                pub to: Vec<Recipient>,
            }
            "#,
        )
        .expect("nothing decodes it on the Dart side");
    }

    /// Guards the premise the strict `fromJson` rests on, against the real file.
    #[test]
    fn the_committed_signals_are_clean() {
        let src = include_str!("../../monero_wasm/src/signals/mod.rs");
        check(src).expect("signals/mod.rs");
    }
}
