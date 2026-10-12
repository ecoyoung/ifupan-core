//! Minimal protobuf3 wire helpers for webcast PushFrame / Response.

#[derive(Clone, Debug)]
pub enum Wire {
    Varint(u64),
    Bytes(Vec<u8>),
    Fixed64(u64),
    Fixed32(u32),
}

pub fn encode_varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
    out
}

pub fn encode_key(field: u32, wt: u32) -> Vec<u8> {
    encode_varint(((field as u64) << 3) | (wt as u64))
}

pub fn encode_len(field: u32, data: &[u8]) -> Vec<u8> {
    let mut out = encode_key(field, 2);
    out.extend(encode_varint(data.len() as u64));
    out.extend_from_slice(data);
    out
}

pub fn encode_string(field: u32, s: &str) -> Vec<u8> {
    encode_len(field, s.as_bytes())
}

pub fn encode_varint_field(field: u32, v: u64) -> Vec<u8> {
    let mut out = encode_key(field, 0);
    out.extend(encode_varint(v));
    out
}

pub fn read_varint(buf: &[u8], i: &mut usize) -> Option<u64> {
    let mut shift = 0;
    let mut n = 0u64;
    while *i < buf.len() {
        let b = buf[*i];
        *i += 1;
        n |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(n);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

pub fn walk(buf: &[u8]) -> Vec<(u32, Wire)> {
    let mut i = 0;
    let mut out = Vec::new();
    while i < buf.len() {
        let key = match read_varint(buf, &mut i) {
            Some(k) => k,
            None => break,
        };
        let field = (key >> 3) as u32;
        let wt = (key & 7) as u32;
        match wt {
            0 => {
                if let Some(v) = read_varint(buf, &mut i) {
                    out.push((field, Wire::Varint(v)));
                } else {
                    break;
                }
            }
            1 => {
                if i + 8 > buf.len() {
                    break;
                }
                let v = u64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
                i += 8;
                out.push((field, Wire::Fixed64(v)));
            }
            2 => {
                let len = match read_varint(buf, &mut i) {
                    Some(n) => n as usize,
                    None => break,
                };
                if i + len > buf.len() {
                    break;
                }
                out.push((field, Wire::Bytes(buf[i..i + len].to_vec())));
                i += len;
            }
            5 => {
                if i + 4 > buf.len() {
                    break;
                }
                let v = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
                i += 4;
                out.push((field, Wire::Fixed32(v)));
            }
            _ => break,
        }
    }
    out
}

pub fn field_bytes<'a>(fields: &'a [(u32, Wire)], n: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|(f, w)| match (f, w) {
        (fn_, Wire::Bytes(b)) if *fn_ == n => Some(b.as_slice()),
        _ => None,
    })
}

pub fn field_str(fields: &[(u32, Wire)], n: u32) -> Option<String> {
    field_bytes(fields, n).map(|b| String::from_utf8_lossy(b).into_owned())
}

pub fn field_varint(fields: &[(u32, Wire)], n: u32) -> Option<u64> {
    fields.iter().find_map(|(f, w)| match (f, w) {
        (fn_, Wire::Varint(v)) if *fn_ == n => Some(*v),
        _ => None,
    })
}

pub fn field_repeated_bytes(fields: &[(u32, Wire)], n: u32) -> Vec<&[u8]> {
    fields
        .iter()
        .filter_map(|(f, w)| match (f, w) {
            (fn_, Wire::Bytes(b)) if *fn_ == n => Some(b.as_slice()),
            _ => None,
        })
        .collect()
}
