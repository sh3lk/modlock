//! wire encodes and decodes the protobuf messages of the boundary between the
//! game and a mod. The generated module calls it; a mod has no need to.

use std::fmt;

/// VARINT is the wire type of the integers, bools and enums.
pub const VARINT: u8 = 0;
/// FIXED64 is the wire type of a double and a fixed64.
pub const FIXED64: u8 = 1;
/// LEN is the wire type of a string, bytes, a message and a packed list.
pub const LEN: u8 = 2;
/// FIXED32 is the wire type of a float and a fixed32.
pub const FIXED32: u8 = 5;

/// Error says why bytes are not the message they were read as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

/// Message is a message of the schema the library holds as a struct.
pub trait Message: Sized {
    /// encode appends the message to out. It leaves out the fields whose
    /// value the receiver assumes.
    fn encode(&self, out: &mut Vec<u8>);

    /// decode returns the message in data. A field it leaves out reads as
    /// its zero value, and a field the library does not know is skipped.
    fn decode(data: &[u8]) -> Result<Self, Error>;
}

/// raw appends values without a tag, as a packed list holds them.
pub mod raw {
    /// varint appends value seven bits at a time, lowest first.
    pub fn varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push(value as u8 | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    /// int32 appends value, in ten bytes when it is negative.
    pub fn int32(out: &mut Vec<u8>, value: i32) {
        varint(out, value as i64 as u64);
    }

    pub fn int64(out: &mut Vec<u8>, value: i64) {
        varint(out, value as u64);
    }

    pub fn uint32(out: &mut Vec<u8>, value: u32) {
        varint(out, value as u64);
    }

    pub fn uint64(out: &mut Vec<u8>, value: u64) {
        varint(out, value);
    }

    pub fn bool(out: &mut Vec<u8>, value: bool) {
        out.push(value as u8);
    }

    pub fn float(out: &mut Vec<u8>, value: f32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    pub fn double(out: &mut Vec<u8>, value: f64) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    pub fn fixed32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    pub fn fixed64(out: &mut Vec<u8>, value: u64) {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// tag appends the key that precedes the value of field number.
fn tag(out: &mut Vec<u8>, number: u32, kind: u8) {
    raw::varint(out, (number as u64) << 3 | kind as u64);
}

pub fn int32(out: &mut Vec<u8>, number: u32, value: i32) {
    tag(out, number, VARINT);
    raw::int32(out, value);
}

pub fn int64(out: &mut Vec<u8>, number: u32, value: i64) {
    tag(out, number, VARINT);
    raw::int64(out, value);
}

pub fn uint32(out: &mut Vec<u8>, number: u32, value: u32) {
    tag(out, number, VARINT);
    raw::uint32(out, value);
}

pub fn uint64(out: &mut Vec<u8>, number: u32, value: u64) {
    tag(out, number, VARINT);
    raw::uint64(out, value);
}

pub fn bool(out: &mut Vec<u8>, number: u32, value: bool) {
    tag(out, number, VARINT);
    raw::bool(out, value);
}

pub fn float(out: &mut Vec<u8>, number: u32, value: f32) {
    tag(out, number, FIXED32);
    raw::float(out, value);
}

pub fn double(out: &mut Vec<u8>, number: u32, value: f64) {
    tag(out, number, FIXED64);
    raw::double(out, value);
}

pub fn fixed32(out: &mut Vec<u8>, number: u32, value: u32) {
    tag(out, number, FIXED32);
    raw::fixed32(out, value);
}

pub fn fixed64(out: &mut Vec<u8>, number: u32, value: u64) {
    tag(out, number, FIXED64);
    raw::fixed64(out, value);
}

pub fn string(out: &mut Vec<u8>, number: u32, value: &str) {
    bytes(out, number, value.as_bytes());
}

pub fn bytes(out: &mut Vec<u8>, number: u32, value: &[u8]) {
    tag(out, number, LEN);
    raw::varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

/// nested appends field number holding what write appends, behind its
/// length. It keeps one byte for the length, which fits a body under 128
/// bytes, and moves a longer body up to make room for the rest.
pub fn nested(out: &mut Vec<u8>, number: u32, write: impl FnOnce(&mut Vec<u8>)) {
    tag(out, number, LEN);
    let at = out.len();
    out.push(0);
    write(out);
    let size = out.len() - at - 1;
    if size < 0x80 {
        out[at] = size as u8;
        return;
    }
    let mut length = Vec::with_capacity(5);
    raw::varint(&mut length, size as u64);
    out.splice(at..=at, length);
}

/// message appends field number holding message.
pub fn message<M: Message>(out: &mut Vec<u8>, number: u32, message: &M) {
    nested(out, number, |out| message.encode(out));
}

/// packed appends field number holding values in one record, each as put
/// appends it. An empty list appends nothing.
pub fn packed<T>(out: &mut Vec<u8>, number: u32, values: &[T], put: impl Fn(&mut Vec<u8>, &T)) {
    if values.is_empty() {
        return;
    }
    nested(out, number, |out| {
        for value in values {
            put(out, value);
        }
    });
}

/// Reader reads the fields of one encoded message in order.
pub struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, at: 0 }
    }

    /// rest returns what the reader has not read yet.
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.at..]
    }

    /// field returns the number and wire type of the next field, whose value
    /// the caller then reads or skips, or None at the end of the message.
    pub fn field(&mut self) -> Result<Option<(u32, u8)>, Error> {
        if self.at >= self.data.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        Ok(Some(((key >> 3) as u32, (key & 7) as u8)))
    }

    fn varint(&mut self) -> Result<u64, Error> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let Some(&byte) = self.data.get(self.at) else {
                return Err(Error("a message ends inside a number"));
            };
            self.at += 1;
            if shift < 64 {
                value |= ((byte & 0x7f) as u64) << shift;
            }
            if byte < 0x80 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                return Err(Error("a number is longer than ten bytes"));
            }
        }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let end = self
            .at
            .checked_add(size)
            .filter(|&end| end <= self.data.len());
        let Some(end) = end else {
            return Err(Error("a message ends inside a field"));
        };
        let taken = &self.data[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn expect(kind: u8, want: u8) -> Result<(), Error> {
        if kind == want {
            Ok(())
        } else {
            Err(Error(
                "a field has another wire type than the schema gives it",
            ))
        }
    }

    pub fn int32(&mut self, kind: u8) -> Result<i32, Error> {
        Self::expect(kind, VARINT)?;
        Ok(self.varint()? as i32)
    }

    pub fn int64(&mut self, kind: u8) -> Result<i64, Error> {
        Self::expect(kind, VARINT)?;
        Ok(self.varint()? as i64)
    }

    pub fn uint32(&mut self, kind: u8) -> Result<u32, Error> {
        Self::expect(kind, VARINT)?;
        Ok(self.varint()? as u32)
    }

    pub fn uint64(&mut self, kind: u8) -> Result<u64, Error> {
        Self::expect(kind, VARINT)?;
        self.varint()
    }

    pub fn bool(&mut self, kind: u8) -> Result<bool, Error> {
        Self::expect(kind, VARINT)?;
        Ok(self.varint()? != 0)
    }

    pub fn float(&mut self, kind: u8) -> Result<f32, Error> {
        Ok(f32::from_bits(self.fixed32(kind)?))
    }

    pub fn double(&mut self, kind: u8) -> Result<f64, Error> {
        Ok(f64::from_bits(self.fixed64(kind)?))
    }

    pub fn fixed32(&mut self, kind: u8) -> Result<u32, Error> {
        Self::expect(kind, FIXED32)?;
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn fixed64(&mut self, kind: u8) -> Result<u64, Error> {
        Self::expect(kind, FIXED64)?;
        let bytes = self.take(8)?;
        let mut value = [0u8; 8];
        value.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(value))
    }

    /// bytes returns the value of a field that travels behind its length:
    /// bytes, or an encoded message.
    pub fn bytes(&mut self, kind: u8) -> Result<&'a [u8], Error> {
        Self::expect(kind, LEN)?;
        let size = self.varint()?;
        self.take(size as usize)
    }

    pub fn string(&mut self, kind: u8) -> Result<String, Error> {
        match std::str::from_utf8(self.bytes(kind)?) {
            Ok(text) => Ok(text.to_owned()),
            Err(_) => Err(Error("a string is not UTF-8")),
        }
    }

    /// skip passes over the value of a field of wire type kind.
    pub fn skip(&mut self, kind: u8) -> Result<(), Error> {
        match kind {
            VARINT => self.varint().map(drop),
            FIXED64 => self.take(8).map(drop),
            LEN => self.bytes(LEN).map(drop),
            FIXED32 => self.take(4).map(drop),
            _ => Err(Error("a message holds a wire type protobuf no longer uses")),
        }
    }

    /// repeated appends to values what a repeated field of wire type kind
    /// holds: each value of a packed record, which a field whose values have
    /// the wire type single may travel as, or else its one value. read reads
    /// one value.
    pub fn repeated<T>(
        &mut self,
        kind: u8,
        single: u8,
        values: &mut Vec<T>,
        read: impl Fn(&mut Reader<'a>, u8) -> Result<T, Error>,
    ) -> Result<(), Error> {
        if kind != LEN || single == LEN {
            values.push(read(self, kind)?);
            return Ok(());
        }
        let mut packed = Reader::new(self.bytes(LEN)?);
        while packed.at < packed.data.len() {
            values.push(read(&mut packed, single)?);
        }
        Ok(())
    }
}
