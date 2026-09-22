#!/usr/bin/env python3
"""Build pktctl's IPC index from the Java framework shipped with Packet Tracer.

Usage: generate.py <pt-cep-java-framework.jar> <output.json> [<javadoc.zip>]

Needs `javap` (any JDK). The index lists every IPC class, its methods with the
exact PTMP type of each argument (read from the *Impl bytecode, not guessed)
and every enum with its wire values. With the Javadoc zip that ships next to
the jar it also records parameter names and each method's summary.
"""

import html

import json
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

IPC_PREFIX = "com/cisco/pt/ipc/"
RESPONSE_FACTORY = "com/cisco/pt/impl/IPCResponseFactory.class"
ENCODERS = {
    "addBoolParameter": "bool",
    "addByteParameter": "byte",
    "addByteListParameter": "bytes",
    "addDoubleParameter": "double",
    "addFloatParameter": "float",
    "addIntParameter": "int",
    "addIPAddressParameter": "ip",
    "addIPV6AddressParameter": "ipv6",
    "addLongParameter": "long",
    "addMACAddressParameter": "mac",
    "addQStringParameter": "qstring",
    "addShortParameter": "short",
    "addStringParameter": "string",
    "addUUIDParameter": "uuid",
}
JAVA_RETURNS = {
    "void": "void",
    "boolean": "bool",
    "java.lang.Boolean": "bool",
    "byte": "byte",
    "java.lang.Byte": "byte",
    "short": "short",
    "java.lang.Short": "short",
    "int": "int",
    "java.lang.Integer": "int",
    "long": "long",
    "java.lang.Long": "long",
    "float": "float",
    "double": "double",
    "java.lang.String": "string",
    "com.cisco.pt.IPAddress": "ip",
    "com.cisco.pt.IPV6Address": "ipv6",
    "com.cisco.pt.MACAddress": "mac",
    "com.cisco.pt.UUID": "uuid",
}
HEADER = re.compile(r"^\s*public (?:abstract )?(?:static )?(?:final )?(.+?) (\w+)\((.*)\)(?: throws .*)?;$")
DECLARATION = re.compile(r"^public (?:abstract )?(interface|class|final class) ([\w.$]+)(?: extends ([\w.$<>, ]+?))?(?: implements ([\w.$<>, ]+))? \{$")
INT_PUSH = re.compile(r"(iconst_m1|iconst_(\d)|bipush\s+(-?\d+)|sipush\s+(-?\d+)|ldc(?:_w)?\s+#\d+\s+// int (-?\d+))")


def javap(root, classes, *flags):
    out = []
    for start in range(0, len(classes), 150):
        batch = classes[start:start + 150]
        out.append(subprocess.run(
            ["javap", "-p", *flags, "-classpath", str(root), *batch],
            check=True, capture_output=True, text=True,
        ).stdout)
    return "\n".join(out)


def short(java):
    return java.split(".")[-1]


def split_blocks(text):
    blocks, current = [], []
    for line in text.splitlines():
        if line.startswith("Compiled from") and current:
            blocks.append(current)
            current = []
        current.append(line)
    if current:
        blocks.append(current)
    return blocks


def int_value(match):
    if match.group(1) == "iconst_m1":
        return -1
    return int(next(group for group in match.groups()[1:] if group is not None))


DESCRIPTOR_TYPES = {"Z": "boolean", "B": "byte", "S": "short", "I": "int", "J": "long", "F": "float", "D": "double", "C": "char"}


def descriptor_params(descriptor):
    inner, types, index = descriptor[1:descriptor.index(")")], [], 0
    while index < len(inner):
        char = inner[index]
        if char == "L":
            end = inner.index(";", index)
            types.append(inner[index + 1:end].replace("/", "."))
            index = end + 1
        elif char == "[":
            index += 1
        else:
            types.append(DESCRIPTOR_TYPES[char])
            index += 1
    return tuple(types)


def method_blocks(block):
    current, body = None, []
    for line in block:
        header = HEADER.match(line)
        if header and not line.startswith("    "):
            if current:
                yield current, body
            ret, name, params = header.groups()
            current, body = (name, tuple(p.strip() for p in params.split(",") if p.strip()), ret), []
        elif current:
            body.append(line)
    if current:
        yield current, body


def encoders_in(body):
    params, enum_arg = [], None
    for line in body:
        getter = re.search(r"Method com/cisco/pt/ipc/enums/(\w+)\.get\w*Value", line)
        if getter:
            enum_arg = getter.group(1)
        encoder = re.search(r"IPCCall\.(add\w+Parameter)", line)
        if encoder:
            kind = ENCODERS[encoder.group(1)]
            params.append(("enum:" + enum_arg) if (enum_arg and kind == "int") else kind)
            enum_arg = None
    return params


def with_enums(params, java_params):
    typed = []
    for kind, java in zip(params, java_params):
        name = short(java)
        typed.append(("enum:" + name) if (kind == "int" and ".enums." in java) else kind)
    return typed


ANCHOR = re.compile(r'<a name="(\w+)-([^"]*)">')


def docs_from(zip_path):
    docs = {}
    if not zip_path:
        return docs
    with zipfile.ZipFile(zip_path) as archive:
        for name in archive.namelist():
            if "/com/cisco/pt/ipc/" not in name or not name.endswith(".html") or "/class-use/" in name:
                continue
            page = archive.read(name).decode("utf-8", "replace")
            owner = name.rsplit("/", 1)[-1][:-5]
            anchors = list(ANCHOR.finditer(page))
            for index, anchor in enumerate(anchors):
                end = anchors[index + 1].start() if index + 1 < len(anchors) else len(page)
                section = page[anchor.start():end]
                signature = re.search(r"<pre>(.*?)</pre>", section, re.S)
                if not signature or "<h4>" not in section:
                    continue
                text = html.unescape(re.sub(r"<[^>]+>", "", signature.group(1)))
                inside = text[text.find("(") + 1:text.rfind(")")]
                names = [part.split()[-1] for part in inside.split(",") if part.strip()]
                brief = re.search(r"\\brief(.*?)(?:\\param|\\return|</pre>|$)", section, re.S)
                summary = " ".join(html.unescape(re.sub(r"<[^>]+>", "", brief.group(1))).split()) if brief else ""
                docs.setdefault((owner, anchor.group(1), len(names)), (names, summary[:400]))
    return docs


READERS = {
    "readBoolean": "bool", "readByte": "byte", "readIPCData": "data", "readDouble": "double",
    "readFloat": "float", "readInt": "int", "readIPAddress": "ip", "readIPV6Address": "ipv6",
    "readLong": "long", "readMACAddress": "mac", "readPair": "pair", "readShort": "short",
    "readQString": "qstring", "readString": "string", "readUUID": "uuid", "readVector": "list",
}


def data_layouts(root, impls, classes):
    factory = javap(root, ["com.cisco.pt.impl.IPCResponseFactory"], "-c")
    wire = {}
    pending = None
    for line in factory.splitlines():
        name = re.search(r"ldc(?:_w)?\s+#\d+\s+// String (\w+)$", line)
        if name:
            pending = name.group(1)
        created = re.search(r"new\s+#\d+\s+// class ([\w/$]+Impl)$", line)
        if created and pending:
            wire[created.group(1).replace("/", ".")] = pending
            pending = None

    raw, variable = {}, set()
    impl_classes = [name for name in classes if name.endswith("Impl")]
    for block in split_blocks(javap(root, impl_classes, "-c")):
        declaration = next((DECLARATION.match(line) for line in block if DECLARATION.match(line)), None)
        if not declaration:
            continue
        java_name = declaration.group(2)
        for (method, params, _), body in method_blocks(block):
            if method != "read" or len(params) != 1:
                continue
            entries = []
            for index, line in enumerate(body):
                parent = re.search(r"invokespecial\s+#\d+\s+// Method ([\w/$]+Impl)\.read:", line)
                if parent:
                    entries.append(("super", parent.group(1).replace("/", ".")))
                    continue
                reader = re.search(r"Method (read\w+):", line)
                if reader and reader.group(1) in READERS:
                    name = None
                    for follow in body[index + 1:index + 4]:
                        stored = re.search(r"putfield\s+#\d+\s+// Field (\w+):", follow)
                        if stored:
                            name = stored.group(1)
                            break
                    entries.append(("field", {"name": name, "kind": READERS[reader.group(1)]}))
                jump = re.search(r"^\s*(\d+): goto\s+(\d+)", line)
                if jump and int(jump.group(2)) < int(jump.group(1)) and any(
                    re.search(r"Method read\w+:", later) for later in body[index:]
                ):
                    variable.add(java_name)
            raw[java_name] = entries

    def resolve(java_name, seen=()):
        fields, loops = [], java_name in variable
        for kind, value in raw.get(java_name, []):
            if kind == "super" and value not in seen:
                inherited, inherited_loops = resolve(value, seen + (java_name,))
                fields.extend(inherited)
                loops = loops or inherited_loops
            elif kind == "field":
                fields.append(dict(value))
        return fields, loops

    layouts = {}
    for java_name, wire_name in wire.items():
        fields, loops = resolve(java_name)
        for position, field in enumerate(fields):
            if not field["name"]:
                field["name"] = f"field{position}"
        interfaces = impls.get(java_name, [])
        layouts[wire_name] = {
            "interface": interfaces[0] if interfaces else short(java_name)[:-4],
            "fields": fields,
            **({"variable": True} if loops else {}),
        }
    return layouts


def returns(java, interfaces, enums):
    base = java.split("<")[0]
    if base in JAVA_RETURNS:
        return JAVA_RETURNS[base]
    if base in ("java.util.Vector", "java.util.List", "java.util.ArrayList"):
        inner = java[java.index("<") + 1:-1] if "<" in java else "?"
        return "list<" + returns(inner, interfaces, enums) + ">"
    if base == "com.cisco.pt.util.Pair":
        return "pair"
    name = short(base)
    if name in enums:
        return "enum:" + name
    if name in interfaces:
        return "object:" + name
    return "java:" + base


def main(jar, output, javadoc=None):
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        with zipfile.ZipFile(jar) as archive:
            names = [name for name in archive.namelist() if name.startswith(IPC_PREFIX) and name.endswith(".class")]
            for name in names + [RESPONSE_FACTORY]:
                archive.extract(name, root)
        classes = sorted(name[:-6].replace("/", ".") for name in names if "$" not in name)

        interfaces, enums, impls = {}, {}, {}
        for block in split_blocks(javap(root, classes)):
            declaration = next((DECLARATION.match(line) for line in block if DECLARATION.match(line)), None)
            if not declaration:
                continue
            kind, java_name, extends, implements = declaration.groups()
            name = short(java_name)
            if kind == "interface" and ".impl." not in java_name:
                parents = [short(p.strip()) for p in (extends or "").split(",") if p.strip()]
                methods = []
                for line in block:
                    header = HEADER.match(line)
                    if header:
                        ret, method, params = header.groups()
                        methods.append((method, [p.strip() for p in params.split(",") if p.strip()], ret))
                interfaces[name] = {"java": java_name, "extends": parents, "raw": methods}
            elif extends and extends.startswith("java.lang.Enum"):
                enums[name] = {}
            elif name.endswith("Impl") and implements:
                impls[java_name] = [short(i.strip()) for i in implements.split(",")]

        enum_classes = [interfaces_java for interfaces_java in classes if short(interfaces_java) in enums]
        for block in split_blocks(javap(root, enum_classes, "-c", "-constants")):
            declaration = next((DECLARATION.match(line) for line in block if DECLARATION.match(line)), None)
            name = short(declaration.group(2))
            lines = block[next(i for i, line in enumerate(block) if "static {};" in line):]
            for index, line in enumerate(lines):
                constant = re.search(r"ldc(?:_w)?\s+#\d+\s+// String (\w+)$", line)
                if not constant:
                    continue
                pushes = []
                for follow in lines[index + 1:index + 6]:
                    match = INT_PUSH.search(follow)
                    if match:
                        pushes.append(int_value(match))
                    if "invokespecial" in follow:
                        break
                if len(pushes) >= 2:
                    enums[name][constant.group(1)] = pushes[1]
                elif len(pushes) == 1:
                    enums[name][constant.group(1)] = pushes[0]

        factory_text = javap(root, ["com.cisco.pt.ipc.IPCFactory"], "-c")
        builders, factory = {}, {}
        for (name, params, _), body in method_blocks(split_blocks(factory_text)[0]):
            if name.startswith("create") and name.endswith("Message"):
                builders[(name, params)] = encoders_in(body)
        for (name, params, _), body in method_blocks(split_blocks(factory_text)[0]):
            if (name, params) in builders:
                continue
            ipc_name, used = None, None
            for line in body:
                call = re.search(r"ldc(?:_w)?\s+#\d+\s+// String (\w+)$", line)
                if call and ipc_name is None:
                    ipc_name = call.group(1)
                builder = re.search(r"Method (create\w*Message):(\([^)]*\))", line)
                if builder:
                    used = builders.get((builder.group(1), descriptor_params(builder.group(2))))
            if ipc_name and used is not None:
                factory[(name, params)] = (ipc_name, with_enums(used, params[1:]))

        wire = {}
        for block in split_blocks(javap(root, sorted(impls), "-c")):
            declaration = next((DECLARATION.match(line) for line in block if DECLARATION.match(line)), None)
            targets = impls.get(declaration.group(2), [])
            for (method, java_params, _), body in method_blocks(block):
                found = None
                for line in body:
                    delegated = re.search(r"Method com/cisco/pt/ipc/IPCFactory\.(\w+):(\([^)]*\))", line)
                    if delegated:
                        found = factory.get((delegated.group(1), descriptor_params(delegated.group(2))))
                        break
                if found is None:
                    ipc_name = next((m.group(1) for m in (re.search(r"ldc(?:_w)?\s+#\d+\s+// String (\w+)$", l) for l in body) if m), None)
                    if ipc_name is None or not any("IPCCall" in l for l in body):
                        continue
                    found = (ipc_name, encoders_in(body))
                for target in targets:
                    wire.setdefault(target, {}).setdefault(method, []).append(found)

        docs = docs_from(javadoc)
        layouts = data_layouts(root, impls, classes)
        index = {"classes": {}, "enums": dict(sorted(enums.items())), "roots": {}, "data": dict(sorted(layouts.items()))}
        for name, info in sorted(interfaces.items()):
            methods = []
            overloads = {}
            for method, java_params, ret in info["raw"]:
                seen = overloads.setdefault(method, 0)
                overloads[method] += 1
                encodings = wire.get(name, {}).get(method, [])
                matching = [(ipc, params) for ipc, params in encodings if len(params) == len(java_params)]
                ipc_name, params = matching[0] if matching else (method, None)
                if params is None:
                    params = ["?" + short(p) for p in java_params]
                methods.append({
                    "name": method,
                    **({"ipc": ipc_name} if ipc_name != method else {}),
                    **({} if matching else {"local": True}),
                    "params": params,
                    "java": [short(p) for p in java_params],
                    "returns": returns(ret, interfaces, enums),
                })
                names, summary = docs.get((name, method, len(java_params)), ([], ""))
                if len(names) == len(java_params) and names:
                    methods[-1]["names"] = names
                if summary:
                    methods[-1]["doc"] = summary
            index["classes"][name] = {"extends": info["extends"], "methods": methods}

        def remote(name, seen=()):
            if name == "IPCObject":
                return True
            parents = index["classes"].get(name, {}).get("extends", [])
            return any(remote(parent, seen + (name,)) for parent in parents if parent not in seen)

        for name, info in index["classes"].items():
            info["remote"] = remote(name)
        for method in index["classes"].get("IPC", {}).get("methods", []):
            if not method["params"] and method["returns"].startswith("object:"):
                index["roots"][method["name"]] = method["returns"][len("object:"):]

    Path(output).write_text(json.dumps(index, separators=(",", ":"), sort_keys=False) + "\n")
    methods = sum(len(c["methods"]) for c in index["classes"].values())
    unresolved = sum(1 for c in index["classes"].values() if c["remote"] for m in c["methods"] for p in m["params"] if p.startswith("?"))
    print(f"{len(index['classes'])} classes, {methods} methods, {len(index['enums'])} enums, "
          f"{len(index['roots'])} roots, {sum(c['remote'] for c in index['classes'].values())} remote classes, "
          f"{unresolved} unresolved remote params, {len(index['data'])} data layouts "
          f"({sum(1 for layout in index['data'].values() if layout.get('variable'))} variable)", file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:4])
