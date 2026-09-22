//! Turns descriptors and generic signatures into the Java type names the index
//! records: `java.lang.String`, `int[]`, `java.util.Vector<com.cisco.pt.ipc.sim.Device>`.

/// The parameter types and return type of a method, read from its generic
/// signature when the compiler recorded one and from its descriptor otherwise.
pub fn method(descriptor: &str, signature: Option<&str>) -> (Vec<String>, String) {
    signature
        .and_then(parse_method)
        .unwrap_or_else(|| parse_method(descriptor).unwrap_or_default())
}

/// The parameter types of a descriptor, ignoring generics.
pub fn parameters(descriptor: &str) -> Vec<String> {
    parse_method(descriptor).unwrap_or_default().0
}

fn parse_method(text: &str) -> Option<(Vec<String>, String)> {
    let open = text.find('(')?;
    let mut at = open + 1;
    let bytes = text.as_bytes();
    let mut parameters = Vec::new();
    while at < bytes.len() && bytes[at] != b')' {
        let (name, next) = one(text, at)?;
        parameters.push(name);
        at = next;
    }
    let (returns, _) = one(text, at + 1)?;
    Some((parameters, returns))
}

/// One type, and where it ends.
fn one(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let primitive = |name: &str| Some((name.to_owned(), at + 1));
    match *bytes.get(at)? {
        b'V' => primitive("void"),
        b'Z' => primitive("boolean"),
        b'B' => primitive("byte"),
        b'C' => primitive("char"),
        b'S' => primitive("short"),
        b'I' => primitive("int"),
        b'J' => primitive("long"),
        b'F' => primitive("float"),
        b'D' => primitive("double"),
        b'[' => {
            let (inner, next) = one(text, at + 1)?;
            Some((format!("{inner}[]"), next))
        }
        b'T' => {
            let end = text[at..].find(';')? + at;
            Some((text[at + 1..end].to_owned(), end + 1))
        }
        b'*' => Some(("?".to_owned(), at + 1)),
        b'+' | b'-' => one(text, at + 1),
        b'L' => object(text, at),
        _ => None,
    }
}

/// A class type, with its type arguments when the signature carries them.
fn object(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let mut end = at + 1;
    while end < bytes.len() && bytes[end] != b';' && bytes[end] != b'<' {
        end += 1;
    }
    let name = text[at + 1..end].replace(['/', '$'], ".");
    if bytes.get(end) == Some(&b'<') {
        let mut arguments = Vec::new();
        let mut inside = end + 1;
        while bytes.get(inside).is_some_and(|byte| *byte != b'>') {
            let (argument, next) = one(text, inside)?;
            arguments.push(argument);
            inside = next;
        }
        let after = inside + 1;
        let close = text[after..].find(';')? + after;
        return Some((format!("{name}<{}>", arguments.join(",")), close + 1));
    }
    Some((name, end + 1))
}

/// The last segment of a dotted name, which is how the index records Java types.
pub fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_descriptors() {
        let (parameters, returns) = method("(Ljava/lang/String;IZ)V", None);
        assert_eq!(parameters, ["java.lang.String", "int", "boolean"]);
        assert_eq!(returns, "void");
        assert_eq!(method("()[B", None).1, "byte[]");
    }

    #[test]
    fn prefers_the_generic_signature() {
        let (_, returns) = method(
            "()Ljava/util/Vector;",
            Some("()Ljava/util/Vector<Lcom/cisco/pt/ipc/sim/ARPEntry;>;"),
        );
        assert_eq!(returns, "java.util.Vector<com.cisco.pt.ipc.sim.ARPEntry>");
    }

    #[test]
    fn writes_inner_classes_with_dots() {
        assert_eq!(
            method("(Lcom/cisco/pt/PopupMenuItem$Inner;)V", None).0,
            ["com.cisco.pt.PopupMenuItem.Inner"]
        );
    }
}
