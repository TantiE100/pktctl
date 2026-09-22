//! Walks the bytecode of a method and reports the few instructions the index
//! needs: constants being pushed, calls, field stores and backward jumps.

use crate::classfile::{Member, Pool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insn {
    /// `ldc` of a string constant.
    Text(String),
    /// Any instruction that pushes a small integer, including `ldc` of one.
    Int(i64),
    Call(Member),
    /// `invokespecial`, which is how a `read` method calls the one it extends.
    Special(Member),
    PutField(String),
    New(String),
    /// A jump, with where it jumps from and to.
    Jump {
        from: u32,
        to: u32,
    },
    Other,
}

/// Every instruction of a method, in order.
pub fn walk(code: &[u8], pool: &Pool) -> Vec<Insn> {
    let mut insns = Vec::new();
    let mut at = 0usize;
    while at < code.len() {
        let opcode = code[at];
        let operand16 = |offset: usize| -> u16 {
            if at + offset + 1 < code.len() {
                u16::from_be_bytes([code[at + offset], code[at + offset + 1]])
            } else {
                0
            }
        };
        let insn = match opcode {
            0x02 => Insn::Int(-1),
            0x03..=0x08 => Insn::Int(i64::from(opcode) - 0x03),
            0x10 => Insn::Int(i64::from(i8::from_be_bytes([code[at + 1]]))),
            0x11 => Insn::Int(i64::from(signed16(code, at + 1))),
            0x12 | 0x13 => {
                let index = if opcode == 0x12 {
                    u16::from(code[at + 1])
                } else {
                    operand16(1)
                };
                pool.string(index).map_or_else(
                    || pool.integer(index).map_or(Insn::Other, Insn::Int),
                    |text| Insn::Text(text.to_owned()),
                )
            }
            0xb5 => pool
                .member(operand16(1))
                .map_or(Insn::Other, |field| Insn::PutField(field.name)),
            0xb7 => pool.member(operand16(1)).map_or(Insn::Other, Insn::Special),
            0xb6 | 0xb8 | 0xb9 => pool.member(operand16(1)).map_or(Insn::Other, Insn::Call),
            0xbb => pool.class_name(operand16(1)).map_or(Insn::Other, Insn::New),
            0xa7 => jump(at, i64::from(signed16(code, at + 1))),
            0xc8 => {
                let offset = code.get(at + 1..at + 5).map_or(0, |bytes| {
                    i64::from(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                });
                jump(at, offset)
            }
            _ => Insn::Other,
        };
        insns.push(insn);
        at += length(code, at);
    }
    insns
}

fn signed16(code: &[u8], at: usize) -> i16 {
    code.get(at..at + 2)
        .map_or(0, |bytes| i16::from_be_bytes([bytes[0], bytes[1]]))
}

fn jump(at: usize, offset: i64) -> Insn {
    let from = u32::try_from(at).unwrap_or(u32::MAX);
    let to = i64::try_from(at)
        .ok()
        .and_then(|at| u32::try_from(at + offset).ok())
        .unwrap_or_default();
    Insn::Jump { from, to }
}

/// How many bytes one instruction takes, so the walk stays in step with the code.
fn length(code: &[u8], at: usize) -> usize {
    match code[at] {
        0x10 | 0x12 | 0x15..=0x19 | 0x36..=0x3a | 0xa9 | 0xbc => 2,
        0x11
        | 0x13
        | 0x14
        | 0x84
        | 0x99..=0xa8
        | 0xb2..=0xb8
        | 0xbb
        | 0xbd
        | 0xc0
        | 0xc1
        | 0xc6
        | 0xc7 => 3,
        0xc5 => 4,
        0xb9 | 0xba | 0xc8 | 0xc9 => 5,
        0xc4 => {
            if code.get(at + 1) == Some(&0x84) {
                6
            } else {
                4
            }
        }
        0xaa => switch_length(code, at, true),
        0xab => switch_length(code, at, false),
        _ => 1,
    }
}

/// `tableswitch` and `lookupswitch` are padded to a four-byte boundary and then
/// carry a variable number of entries.
fn switch_length(code: &[u8], at: usize, table: bool) -> usize {
    let read = |offset: usize| -> i64 {
        code.get(offset..offset + 4).map_or(0, |bytes| {
            i64::from(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        })
    };
    let count = |value: i64| usize::try_from(value.max(0)).unwrap_or_default();
    let padding = 3 - (at % 4);
    let body = at + 1 + padding;
    if table {
        let (low, high) = (read(body + 4), read(body + 8));
        1 + padding + 12 + count(high - low + 1) * 4
    } else {
        1 + padding + 8 + count(read(body + 4)) * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_over_every_instruction_shape() {
        // iconst_1, bipush 7, sipush 300, goto -3, return
        let code = [0x04, 0x10, 0x07, 0x11, 0x01, 0x2c, 0xa7, 0xff, 0xfd, 0xb1];
        let insns = walk(&code, &Pool::default());
        assert_eq!(
            insns,
            [
                Insn::Int(1),
                Insn::Int(7),
                Insn::Int(300),
                Insn::Jump { from: 6, to: 3 },
                Insn::Other,
            ]
        );
    }

    #[test]
    fn measures_a_padded_lookupswitch() {
        let mut code = vec![
            0x00, 0xab, 0x00, 0x00, 0, 0, 0, 9, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2,
        ];
        code.push(0xb1);
        assert_eq!(length(&code, 1), 1 + 2 + 8 + 8);
    }
}
