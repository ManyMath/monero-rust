#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Dart2Rust,
    Rust2Dart,
    Shared,
}

#[derive(Debug, Clone)]
pub enum RustType {
    Str,
    U8,
    U32,
    U64,
    Bool,
    Vec(Box<RustType>),
    Option(Box<RustType>),
    Tuple(Vec<RustType>),
    Named(String),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: RustType,
    pub has_serde_default: bool,
}

#[derive(Debug, Clone)]
pub struct SignalClass {
    pub name: String,
    pub direction: Direction,
    /// Whether the Rust struct derives Serialize
    pub has_serialize: bool,
    /// Whether the Rust struct derives Deserialize
    pub has_deserialize: bool,
    pub fields: Vec<Field>,
}

impl SignalClass {
    /// Returns true if any field has type Option<Tuple(...)>
    pub fn has_tuple_option(&self) -> bool {
        self.fields.iter().any(|f| {
            matches!(&f.ty, RustType::Option(inner) if matches!(inner.as_ref(), RustType::Tuple(_)))
        })
    }
}
