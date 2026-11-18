//! An epee codec for the hostile node harness, independent of the code under test.

#![allow(dead_code)]

pub const HEADER: &[u8] = b"\x01\x11\x01\x01\x01\x01\x02\x01\x01";

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    I64(i64),
    I32(i32),
    I16(i16),
    I8(i8),
    U64(u64),
    U32(u32),
    U16(u16),
    U8(u8),
    F64(f64),
    Str(Vec<u8>),
    Bool(bool),
    Obj(Obj),
    Arr(u8, Vec<Val>),
}

pub type Obj = Vec<(String, Val)>;

impl Val {
    fn tag(&self) -> u8 {
        match self {
            Val::I64(_) => 1,
            Val::I32(_) => 2,
            Val::I16(_) => 3,
            Val::I8(_) => 4,
            Val::U64(_) => 5,
            Val::U32(_) => 6,
            Val::U16(_) => 7,
            Val::U8(_) => 8,
            Val::F64(_) => 9,
            Val::Str(_) => 10,
            Val::Bool(_) => 11,
            Val::Obj(_) => 12,
            Val::Arr(tag, _) => tag | 0x80,
        }
    }

    pub fn u64(&self) -> u64 {
        match self {
            Val::U64(v) => *v,
            Val::U32(v) => u64::from(*v),
            Val::U16(v) => u64::from(*v),
            Val::U8(v) => u64::from(*v),
            Val::I64(v) => *v as u64,
            other => panic!("not an integer: {other:?}"),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Val::Str(v) => v,
            other => panic!("not a string: {other:?}"),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Val::Bool(v) => *v,
            other => panic!("not a bool: {other:?}"),
        }
    }

    pub fn obj(&self) -> &Obj {
        match self {
            Val::Obj(v) => v,
            other => panic!("not an object: {other:?}"),
        }
    }

    pub fn obj_mut(&mut self) -> &mut Obj {
        match self {
            Val::Obj(v) => v,
            other => panic!("not an object: {other:?}"),
        }
    }

    pub fn arr(&self) -> &[Val] {
        match self {
            Val::Arr(_, v) => v,
            other => panic!("not an array: {other:?}"),
        }
    }

    pub fn arr_mut(&mut self) -> &mut Vec<Val> {
        match self {
            Val::Arr(_, v) => v,
            other => panic!("not an array: {other:?}"),
        }
    }
}

pub fn get<'a>(obj: &'a Obj, name: &str) -> Option<&'a Val> {
    obj.iter().find(|(key, _)| key == name).map(|(_, v)| v)
}

pub fn get_mut<'a>(obj: &'a mut Obj, name: &str) -> Option<&'a mut Val> {
    obj.iter_mut().find(|(key, _)| key == name).map(|(_, v)| v)
}

pub fn set(obj: &mut Obj, name: &str, value: Val) {
    if let Some(slot) = get_mut(obj, name) {
        *slot = value;
    } else {
        obj.push((name.to_string(), value));
    }
}

pub fn remove(obj: &mut Obj, name: &str) -> Option<Val> {
    let position = obj.iter().position(|(key, _)| key == name)?;
    Some(obj.remove(position).1)
}

pub fn items<'a>(obj: &'a Obj, name: &str) -> &'a [Val] {
    get(obj, name).map(Val::arr).unwrap_or(&[])
}

pub fn write_varint(value: u64, out: &mut Vec<u8>) {
    if value <= 63 {
        out.push((value << 2) as u8);
    } else if value <= 16383 {
        out.extend_from_slice(&(((value << 2) | 1) as u16).to_le_bytes());
    } else if value <= 1_073_741_823 {
        out.extend_from_slice(&(((value << 2) | 2) as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&((value << 2) | 3).to_le_bytes());
    }
}

fn write_value(value: &Val, out: &mut Vec<u8>) {
    match value {
        Val::I64(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::I32(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::I16(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::I8(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::U64(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::U32(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::U16(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::U8(v) => out.push(*v),
        Val::F64(v) => out.extend_from_slice(&v.to_le_bytes()),
        Val::Str(v) => {
            write_varint(v.len() as u64, out);
            out.extend_from_slice(v);
        }
        Val::Bool(v) => out.push(u8::from(*v)),
        Val::Obj(v) => write_section(v, out),
        Val::Arr(_, items) => {
            write_varint(items.len() as u64, out);
            for item in items {
                write_value(item, out);
            }
        }
    }
}

pub fn write_section(obj: &Obj, out: &mut Vec<u8>) {
    write_varint(obj.len() as u64, out);
    for (name, value) in obj {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.push(value.tag());
        write_value(value, out);
    }
}

pub fn encode(obj: &Obj) -> Vec<u8> {
    let mut out = HEADER.to_vec();
    write_section(obj, &mut out);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        if len > self.bytes.len() {
            return Err("truncated epee".to_string());
        }
        let (head, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(head)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let first = *self.bytes.first().ok_or("truncated epee varint")?;
        let len = 1usize << (first & 3);
        let mut raw = [0u8; 8];
        raw[..len].copy_from_slice(self.take(len)?);
        Ok(u64::from_le_bytes(raw) >> 2)
    }

    fn value(&mut self, tag: u8) -> Result<Val, String> {
        Ok(match tag {
            1 => Val::I64(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            2 => Val::I32(i32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            3 => Val::I16(i16::from_le_bytes(self.take(2)?.try_into().unwrap())),
            4 => Val::I8(self.take(1)?[0] as i8),
            5 => Val::U64(u64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            6 => Val::U32(u32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            7 => Val::U16(u16::from_le_bytes(self.take(2)?.try_into().unwrap())),
            8 => Val::U8(self.take(1)?[0]),
            9 => Val::F64(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            10 => {
                let len = self.varint()? as usize;
                Val::Str(self.take(len)?.to_vec())
            }
            11 => Val::Bool(self.take(1)?[0] != 0),
            12 => Val::Obj(self.section()?),
            other => return Err(format!("unknown epee type {other}")),
        })
    }

    fn section(&mut self) -> Result<Obj, String> {
        let count = self.varint()?;
        let mut obj = Vec::new();
        for _ in 0..count {
            let name_len = self.take(1)?[0] as usize;
            let name = String::from_utf8(self.take(name_len)?.to_vec())
                .map_err(|_| "epee field name is not UTF-8".to_string())?;
            let tag = self.take(1)?[0];
            let value = if tag & 0x80 != 0 {
                let element = tag & 0x7f;
                let count = self.varint()?;
                let mut items = Vec::new();
                for _ in 0..count {
                    items.push(self.value(element)?);
                }
                Val::Arr(element, items)
            } else {
                self.value(tag)?
            };
            obj.push((name, value));
        }
        Ok(obj)
    }
}

pub fn decode(bytes: &[u8]) -> Result<Obj, String> {
    let body = bytes
        .strip_prefix(HEADER)
        .ok_or_else(|| "missing epee header".to_string())?;
    let mut reader = Reader { bytes: body };
    let obj = reader.section()?;
    if !reader.bytes.is_empty() {
        return Err("trailing bytes after epee object".to_string());
    }
    Ok(obj)
}

pub fn str_val(text: &str) -> Val {
    Val::Str(text.as_bytes().to_vec())
}
