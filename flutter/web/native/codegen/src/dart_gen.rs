use crate::model::{Direction, Field, RustType, SignalClass};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate(signals: &[SignalClass]) -> String {
    let mut out = String::new();

    // Header
    out.push_str("import 'dart:async';\n");
    out.push_str("import 'dart:convert';\n");
    out.push_str("import 'signal_sender.dart';\n");
    out.push('\n');
    out.push_str("export 'signal_sender.dart' show setSignalSender;\n");
    out.push('\n');

    // Sub-structs
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push_str("// Sub-structs (shared between multiple signals)\n");
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push('\n');
    for sig in signals.iter().filter(|s| s.direction == Direction::Shared) {
        out.push_str(&gen_shared_class(sig));
    }

    // _send helper
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push_str("// Helper: send a DartSignal to the worker\n");
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push('\n');
    out.push_str("void _send(String fnName, Map<String, dynamic> data) {\n");
    out.push_str("  signalSender.sendSignal(fnName, data);\n");
    out.push_str("}\n");
    out.push('\n');

    // DartSignal types
    let dart_signals: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Dart2Rust)
        .collect();
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push_str(&format!(
        "// DartSignal types (Dart -> Rust, {} total)\n",
        dart_signals.len()
    ));
    out.push_str("// Each has sendSignalToRust().\n");
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push('\n');
    for sig in &dart_signals {
        out.push_str(&gen_dart_signal(sig));
    }

    // RustSignal types
    let rust_signals: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Rust2Dart)
        .collect();
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push_str(&format!(
        "// RustSignal types (Rust -> Dart, {} total)\n",
        rust_signals.len()
    ));
    out.push_str("// Each has fromJson() and a static stream getter.\n");
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    out.push('\n');
    for sig in &rust_signals {
        out.push_str(&gen_rust_signal(sig));
    }

    out
}

// ---------------------------------------------------------------------------
// Shared sub-struct generation
// ---------------------------------------------------------------------------

fn gen_shared_class(sig: &SignalClass) -> String {
    let mut out = String::new();
    out.push_str(&format!("class {} {{\n", sig.name));

    // Field declarations
    for f in &sig.fields {
        out.push_str(&format!(
            "  final {}{};\n",
            dart_type_for_field(f),
            to_camel(&f.name)
        ));
    }

    // Constructor
    if sig.has_deserialize {
        out.push('\n');
        out.push_str(&gen_constructor(&sig.name, &sig.fields));
    } else if sig.fields.is_empty() {
        // Serialize-only with no fields — shouldn't happen but handle gracefully
        out.push('\n');
        out.push_str(&format!("  const {}();\n", sig.name));
    } else {
        // Serialize-only (fromJson-only struct): still need a constructor for fromJson
        out.push('\n');
        out.push_str(&gen_constructor(&sig.name, &sig.fields));
    }

    // toJson if has Deserialize (Dart sends it to Rust)
    if sig.has_deserialize {
        out.push('\n');
        out.push_str(&gen_to_json(&sig.fields));
    }

    // fromJson if has Serialize (Rust sends it to Dart)
    if sig.has_serialize {
        out.push('\n');
        if sig.has_tuple_option() {
            out.push_str(&gen_from_json_block(&sig.name, &sig.fields));
        } else {
            out.push_str(&gen_from_json_arrow(&sig.name, &sig.fields));
        }
    }

    // Special case: OwnedOutput gets copyWith
    if sig.name == "OwnedOutput" {
        out.push('\n');
        out.push_str(&gen_owned_output_copy_with(&sig.fields));
    }

    out.push_str("}\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// DartSignal class generation
// ---------------------------------------------------------------------------

fn gen_dart_signal(sig: &SignalClass) -> String {
    let mut out = String::new();
    out.push_str(&format!("class {} {{\n", sig.name));

    // Field declarations
    for f in &sig.fields {
        out.push_str(&format!(
            "  final {}{};\n",
            dart_type_for_field(f),
            to_camel(&f.name)
        ));
    }

    // Constructor
    if sig.fields.is_empty() {
        out.push_str(&format!("  const {}();\n", sig.name));
    } else {
        out.push_str(&gen_constructor(&sig.name, &sig.fields));
    }

    // sendSignalToRust
    out.push('\n');
    out.push_str(&gen_send_signal_to_rust(sig));

    out.push_str("}\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// RustSignal class generation
// ---------------------------------------------------------------------------

fn gen_rust_signal(sig: &SignalClass) -> String {
    let mut out = String::new();
    out.push_str(&format!("class {} {{\n", sig.name));

    // Field declarations
    for f in &sig.fields {
        out.push_str(&format!(
            "  final {}{};\n",
            dart_type_for_field(f),
            to_camel(&f.name)
        ));
    }

    out.push('\n');

    // Constructor
    if sig.fields.is_empty() {
        out.push_str(&format!("  const {}();\n", sig.name));
    } else {
        out.push_str(&gen_constructor(&sig.name, &sig.fields));
    }

    // fromJson
    out.push('\n');
    if sig.has_tuple_option() {
        out.push_str(&gen_from_json_block(&sig.name, &sig.fields));
    } else {
        out.push_str(&gen_from_json_arrow(&sig.name, &sig.fields));
    }

    // stream getter
    out.push('\n');
    out.push_str(&format!(
        "  static Stream<{name}> get stream =>\n      signalSender.onRawSignal('{name}').map({name}.fromJson);\n",
        name = sig.name
    ));

    out.push_str("}\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Method generators
// ---------------------------------------------------------------------------

fn gen_constructor(name: &str, fields: &[Field]) -> String {
    let use_multi = needs_multiline_constructor(fields);
    if fields.is_empty() {
        return format!("  const {}();\n", name);
    }
    if use_multi {
        let mut out = format!("  const {}({{\n", name);
        for f in fields {
            let camel = to_camel(&f.name);
            let param = constructor_param(f, &camel);
            out.push_str(&format!("    {},\n", param));
        }
        out.push_str("  });\n");
        out
    } else {
        let params: Vec<String> = fields
            .iter()
            .map(|f| {
                let camel = to_camel(&f.name);
                constructor_param(f, &camel)
            })
            .collect();
        format!("  const {}({{{}}});\n", name, params.join(", "))
    }
}

fn constructor_param(f: &Field, camel: &str) -> String {
    if f.has_serde_default {
        let default = dart_default_value(&f.ty);
        format!("this.{}{}", camel, default)
    } else if is_optional_type(&f.ty) {
        format!("this.{}", camel)
    } else {
        format!("required this.{}", camel)
    }
}

fn needs_multiline_constructor(fields: &[Field]) -> bool {
    if fields.len() >= 3 {
        return true;
    }
    fields
        .iter()
        .any(|f| f.has_serde_default || is_optional_type(&f.ty))
}

fn gen_to_json(fields: &[Field]) -> String {
    if fields.is_empty() {
        return "  Map<String, dynamic> toJson() => {};\n".to_string();
    }

    let entries: Vec<String> = fields.iter().map(to_json_entry).collect();

    // Single-line if 1-2 entries and all are non-conditional
    let all_simple = entries.iter().all(|e| !e.starts_with("if "));
    if entries.len() <= 2 && all_simple {
        let joined = entries.join(", ");
        return format!("  Map<String, dynamic> toJson() => {{{}}};\n", joined);
    }

    let mut out = String::from("  Map<String, dynamic> toJson() => {\n");
    for entry in &entries {
        out.push_str(&format!("    {},\n", entry));
    }
    out.push_str("  };\n");
    out
}

fn to_json_entry(f: &Field) -> String {
    let key = &f.name;
    let camel = to_camel(key);
    match &f.ty {
        RustType::Option(inner) => match inner.as_ref() {
            RustType::Tuple(_) => {
                format!(
                    "if ({camel} != null)\n      '{key}': [{camel}!.$1, {camel}!.$2]"
                )
            }
            RustType::Named(_) => {
                format!("if ({camel} != null) '{key}': {camel}!.toJson()")
            }
            _ => format!("if ({camel} != null) '{key}': {camel}"),
        },
        RustType::Vec(inner) => match inner.as_ref() {
            RustType::Named(_) => {
                format!("'{key}': {camel}.map((e) => e.toJson()).toList()")
            }
            _ => format!("'{key}': {camel}"),
        },
        _ => format!("'{key}': {camel}"),
    }
}

fn gen_send_signal_to_rust(sig: &SignalClass) -> String {
    let snake = pascal_to_snake(&sig.name);
    let fn_name = format!("send_{}", snake);

    if sig.fields.is_empty() {
        return format!(
            "  void sendSignalToRust() => _send('{}', {{}});\n",
            fn_name
        );
    }

    // 1-field, non-conditional → compact form
    let entries: Vec<String> = sig.fields.iter().map(to_json_entry).collect();
    let all_simple = entries.iter().all(|e| !e.starts_with("if "));
    if entries.len() == 1 && all_simple {
        return format!(
            "  void sendSignalToRust() =>\n      _send('{}', {{{}}});\n",
            fn_name, entries[0]
        );
    }

    let mut out = format!(
        "  void sendSignalToRust() => _send('{}', {{\n",
        fn_name
    );
    for entry in &entries {
        out.push_str(&format!("    {},\n", entry));
    }
    out.push_str("  });\n");
    out
}

fn gen_from_json_arrow(name: &str, fields: &[Field]) -> String {
    if fields.is_empty() {
        return format!(
            "  factory {name}.fromJson(Map<String, dynamic> json) => {name}();\n"
        );
    }

    let exprs: Vec<String> = fields.iter().map(|f| from_json_expr(f)).collect();

    // Single-line if exactly 1 field with a simple expression
    if exprs.len() == 1 && !exprs[0].contains('\n') {
        return format!(
            "  factory {name}.fromJson(Map<String, dynamic> json) =>\n      {name}({});\n",
            exprs[0]
        );
    }

    let mut out = format!(
        "  factory {name}.fromJson(Map<String, dynamic> json) =>\n      {name}(\n"
    );
    for expr in &exprs {
        out.push_str(&format!("        {},\n", expr));
    }
    out.push_str("      );\n");
    out
}

fn gen_from_json_block(name: &str, fields: &[Field]) -> String {
    // Find the tuple-option field name for the `final si` pre-declaration
    let tuple_field = fields.iter().find(|f| {
        matches!(&f.ty, RustType::Option(inner) if matches!(inner.as_ref(), RustType::Tuple(_)))
    });

    let si_key = tuple_field.map(|f| f.name.as_str()).unwrap_or("subaddress_index");

    let mut out = format!(
        "  factory {name}.fromJson(Map<String, dynamic> json) {{\n"
    );
    out.push_str(&format!(
        "    final si = json['{}'];\n",
        si_key
    ));
    out.push_str(&format!("    return {name}(\n"));
    for f in fields {
        let expr = from_json_expr(f);
        out.push_str(&format!("      {},\n", expr));
    }
    out.push_str("    );\n");
    out.push_str("  }\n");
    out
}

fn from_json_expr(f: &Field) -> String {
    let key = &f.name;
    let camel = to_camel(key);
    match &f.ty {
        RustType::Option(inner) => match inner.as_ref() {
            RustType::Tuple(_) => {
                // Uses the pre-computed `si` variable
                format!(
                    "{camel}:\n          si != null ? ((si as List)[0] as int, (si as List)[1] as int) : null"
                )
            }
            RustType::Named(n) => {
                format!("{camel}: json['{key}'] as {n}?")
            }
            other => {
                let dt = dart_type_simple(other);
                format!("{camel}: json['{key}'] as {dt}?")
            }
        },
        RustType::Vec(inner) => match inner.as_ref() {
            RustType::Named(n) => {
                format!(
                    "{camel}: (json['{key}'] as List)\n            .map((e) => {n}.fromJson(e as Map<String, dynamic>))\n            .toList()"
                )
            }
            other => {
                let dt = dart_type_simple(other);
                format!("{camel}: (json['{key}'] as List).cast<{dt}>()")
            }
        },
        RustType::Bool if f.has_serde_default => {
            format!("{camel}: json['{key}'] as bool? ?? false")
        }
        other => {
            let dt = dart_type_simple(other);
            format!("{camel}: json['{key}'] as {dt}")
        }
    }
}

fn gen_owned_output_copy_with(fields: &[Field]) -> String {
    let mut out =
        String::from("  OwnedOutput copyWith({bool? spent, bool? frozen}) => OwnedOutput(\n");
    for f in fields {
        let camel = to_camel(&f.name);
        let rhs = match f.name.as_str() {
            "spent" => "spent ?? this.spent".to_string(),
            "frozen" => "frozen ?? this.frozen".to_string(),
            _ => camel.clone(),
        };
        out.push_str(&format!("    {}: {},\n", camel, rhs));
    }
    out.push_str("  );\n");
    out
}

// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

pub fn dart_type_for_field(f: &Field) -> String {
    let base = dart_type(&f.ty);
    // Trailing space is included by caller
    format!("{} ", base)
}

pub fn dart_type(ty: &RustType) -> String {
    match ty {
        RustType::Str => "String".into(),
        RustType::U8 | RustType::U32 | RustType::U64 => "int".into(),
        RustType::Bool => "bool".into(),
        RustType::Vec(inner) => format!("List<{}>", dart_type(inner)),
        RustType::Option(inner) => format!("{}?", dart_type(inner)),
        RustType::Tuple(elems) => {
            let parts: Vec<_> = elems.iter().map(dart_type).collect();
            format!("({})", parts.join(", "))
        }
        RustType::Named(n) => n.clone(),
    }
}

fn dart_type_simple(ty: &RustType) -> String {
    dart_type(ty)
}

fn is_optional_type(ty: &RustType) -> bool {
    matches!(ty, RustType::Option(_))
}

fn dart_default_value(ty: &RustType) -> &'static str {
    match ty {
        RustType::Str => " = ''",
        RustType::U8 | RustType::U32 | RustType::U64 => " = 0",
        RustType::Bool => " = false",
        RustType::Vec(_) => " = const []",
        RustType::Option(_) => "",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// Name conversion utilities
// ---------------------------------------------------------------------------

/// Convert snake_case to camelCase
pub fn to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert PascalCase to snake_case
pub fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.extend(ch.to_lowercase());
    }
    result
}
