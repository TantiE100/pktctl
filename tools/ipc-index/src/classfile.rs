//! Reads a Java `.class` file: the constant pool, what the class extends and
//! implements, and every method with its descriptor, generic signature and code.

use std::fmt;

#[derive(Debug)]
pub struct ClassError(String);

impl fmt::Display for ClassError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for ClassError {}

type Result<T> = std::result::Result<T, ClassError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(ClassError(message.into()))
}

const ACC_PUBLIC: u16 = 0x0001;
const ACC_INTERFACE: u16 = 0x0200;

#[derive(Debug, Clone)]
enum Entry {
    Utf8(String),
    Integer(i32),
    /// A class, string or name-and-type: one or two indexes into the pool.
    Indexes(u8, u16, u16),
    Other,
}

/// The constant pool, indexed the way the class file indexes it: from 1.
#[derive(Debug, Default)]
pub struct Pool(Vec<Entry>);

impl Pool {
    fn at(&self, index: u16) -> Result<&Entry> {
        self.0
            .get(index as usize)
            .ok_or_else(|| ClassError(format!("constant {index} is outside the pool")))
    }

    pub fn utf8(&self, index: u16) -> Result<&str> {
        match self.at(index)? {
            Entry::Utf8(text) => Ok(text),
            other => fail(format!("constant {index} is {other:?}, not text")),
        }
    }

    /// The dotted name of a class constant: `com.cisco.pt.ipc.sim.Device`.
    pub fn class_name(&self, index: u16) -> Result<String> {
        match self.at(index)? {
            Entry::Indexes(7, name, _) => Ok(self.utf8(*name)?.replace('/', ".")),
            other => fail(format!("constant {index} is {other:?}, not a class")),
        }
    }

    /// The text of a string constant, or `None` for any other kind of constant.
    pub fn string(&self, index: u16) -> Option<&str> {
        match self.at(index).ok()? {
            Entry::Indexes(8, text, _) => self.utf8(*text).ok(),
            _ => None,
        }
    }

    /// The value of an integer constant, or `None` for any other kind.
    pub fn integer(&self, index: u16) -> Option<i64> {
        match self.at(index).ok()? {
            Entry::Integer(value) => Some(i64::from(*value)),
            _ => None,
        }
    }

    /// The owner, name and descriptor of a field or method reference.
    pub fn member(&self, index: u16) -> Result<Member> {
        match self.at(index)? {
            Entry::Indexes(9..=11, class, name_and_type) => {
                let Entry::Indexes(12, name, descriptor) = self.at(*name_and_type)? else {
                    return fail(format!("constant {name_and_type} is not a name and type"));
                };
                Ok(Member {
                    owner: self.class_name(*class)?,
                    name: self.utf8(*name)?.to_owned(),
                    descriptor: self.utf8(*descriptor)?.to_owned(),
                })
            }
            other => fail(format!("constant {index} is {other:?}, not a member")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Dotted class name, as in `com.cisco.pt.ipc.impl.IPCCall`.
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub descriptor: String,
    /// The generic signature, when the compiler recorded one.
    pub signature: Option<String>,
    pub public: bool,
    pub code: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct Class {
    /// Dotted name, as in `com.cisco.pt.ipc.sim.Device`.
    pub name: String,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub interface: bool,
    pub methods: Vec<Method>,
    pub pool: Pool,
}

/// Reads big-endian numbers off a class file, in order.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.at + count;
        if end > self.bytes.len() {
            return fail("the class file ends in the middle of a structure");
        }
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

fn read_pool(cursor: &mut Cursor) -> Result<Pool> {
    let count = cursor.u16()?;
    let mut entries = vec![Entry::Other];
    while entries.len() < count as usize {
        let tag = cursor.u8()?;
        let entry = match tag {
            1 => {
                let length = cursor.u16()? as usize;
                Entry::Utf8(String::from_utf8_lossy(cursor.take(length)?).into_owned())
            }
            3 => Entry::Integer(i32::from_be_bytes(cursor.take(4)?.try_into().unwrap())),
            4 => {
                cursor.take(4)?;
                Entry::Other
            }
            5 | 6 => {
                cursor.take(8)?;
                Entry::Other
            }
            7 | 8 | 16 | 19..=20 => Entry::Indexes(tag, cursor.u16()?, 0),
            9..=12 | 17 | 18 => Entry::Indexes(tag, cursor.u16()?, cursor.u16()?),
            15 => {
                cursor.u8()?;
                Entry::Indexes(tag, cursor.u16()?, 0)
            }
            other => return fail(format!("constant tag {other} is not one this reader knows")),
        };
        let wide = matches!(tag, 5 | 6);
        entries.push(entry);
        if wide {
            entries.push(Entry::Other);
        }
    }
    Ok(Pool(entries))
}

/// Skips a run of attributes, handing each one to `visit` first.
fn read_attributes(
    cursor: &mut Cursor,
    pool: &Pool,
    mut visit: impl FnMut(&str, &[u8]) -> Result<()>,
) -> Result<()> {
    let count = cursor.u16()?;
    for _ in 0..count {
        let name = pool.utf8(cursor.u16()?)?.to_owned();
        let length = cursor.u32()? as usize;
        let body = cursor.take(length)?;
        visit(&name, body)?;
    }
    Ok(())
}

fn code_of(body: &[u8], pool: &Pool) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(body);
    cursor.take(4)?;
    let length = cursor.u32()? as usize;
    let code = cursor.take(length)?.to_vec();
    let exceptions = cursor.u16()? as usize;
    cursor.take(exceptions * 8)?;
    read_attributes(&mut cursor, pool, |_, _| Ok(()))?;
    Ok(code)
}

fn read_members(cursor: &mut Cursor, pool: &Pool, methods: bool) -> Result<Vec<Method>> {
    let count = cursor.u16()?;
    let mut members = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let flags = cursor.u16()?;
        let name = pool.utf8(cursor.u16()?)?.to_owned();
        let descriptor = pool.utf8(cursor.u16()?)?.to_owned();
        let (mut signature, mut code) = (None, None);
        read_attributes(cursor, pool, |attribute, body| {
            match attribute {
                "Signature" if body.len() == 2 => {
                    let index = u16::from_be_bytes([body[0], body[1]]);
                    signature = Some(pool.utf8(index)?.to_owned());
                }
                "Code" if methods => code = Some(code_of(body, pool)?),
                _ => {}
            }
            Ok(())
        })?;
        members.push(Method {
            name,
            descriptor,
            signature,
            public: flags & ACC_PUBLIC != 0,
            code,
        });
    }
    Ok(members)
}

/// Reads one class file.
pub fn read(bytes: &[u8]) -> Result<Class> {
    let mut cursor = Cursor::new(bytes);
    if cursor.u32()? != 0xCAFE_BABE {
        return fail("this is not a class file");
    }
    cursor.take(4)?;
    let pool = read_pool(&mut cursor)?;
    let flags = cursor.u16()?;
    let name = pool.class_name(cursor.u16()?)?;
    let super_index = cursor.u16()?;
    let super_name = (super_index != 0).then(|| pool.class_name(super_index)).transpose()?;
    let count = cursor.u16()?;
    let mut interfaces = Vec::with_capacity(count as usize);
    for _ in 0..count {
        interfaces.push(pool.class_name(cursor.u16()?)?);
    }
    read_members(&mut cursor, &pool, false)?;
    let methods = read_members(&mut cursor, &pool, true)?;
    Ok(Class {
        name,
        super_name,
        interfaces,
        interface: flags & ACC_INTERFACE != 0,
        methods,
        pool,
    })
}
