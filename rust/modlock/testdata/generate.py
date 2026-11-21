"""generate.py writes testdata/messages.txt: for each message the Rust library
declares, encodings the library must decode and encode back byte for byte. It
builds them from the schema the Python library generates, and checks that the
Python library decodes each one, so the two libraries agree on the wire.

Run it from the repository root after the schema changes:

    python rust/modlock/testdata/generate.py
"""

import pathlib
import random
import re
import struct
import sys
import types

ROOT = pathlib.Path(__file__).resolve().parents[3]
RUST = ROOT / "rust" / "modlock" / "src" / "host.gen.rs"
OUT = pathlib.Path(__file__).with_name("messages.txt")

# The Python library imports the host's module, which only a mod has.
host = types.ModuleType("_modlock")
host.handle = lambda serve: None  # type: ignore[attr-defined]
sys.modules["_modlock"] = host
sys.path.insert(0, str(ROOT / "python"))
import modlock  # noqa: E402
from modlock import wire  # noqa: E402

SCHEMA = modlock._SCHEMA

# How many encodings each message gets.
SAMPLES = 6

# How deep a message may nest its lists and optional members before it leaves
# them out, which ends a message that holds itself.
DEPTH = 4

FLOATS = [0.0, 1.0, -1.5, 0.25, 1024.0, 3.5e10, -0.0078125]
TEXTS = ["", "a", "hero_inferno", "Привет", "two words", "x" * 140]


def declared() -> tuple[list[str], set[str]]:
    """declared returns the schema names of the messages the Rust library
    declares, and of those it holds as an enum of a oneof's cases."""
    source = RUST.read_text(encoding="utf-8")
    names = re.findall(r'^        "(\w+)" => \w+::decode\(data\)', source, re.M)
    rust = {name.replace("_", ""): name for name in names}
    unions = set()
    for name in re.findall(r"^impl (\w+) \{\n    /// encode appends the", source, re.M):
        unions.add(rust[name])
    return names, unions


NAMES, UNIONS = declared()


def holds_itself(name: str) -> set[int]:
    """holds_itself returns the numbers of the single message fields of name
    that hold name again, which the Rust library keeps behind a pointer and so
    may leave out."""

    def reaches(start: str, seen: set[str]) -> bool:
        if start == name:
            return True
        if start in seen:
            return False
        seen.add(start)
        return any(f.message and not f.repeated and reaches(f.message, seen) for f in SCHEMA[start][1])

    return {f.number for f in SCHEMA[name][1] if f.message and not f.repeated and reaches(f.message, set())}


def varint(value: int) -> bytes:
    value &= 0xFFFFFFFFFFFFFFFF
    out = bytearray()
    while value >= 0x80:
        out.append(value & 0x7F | 0x80)
        value >>= 7
    out.append(value)
    return bytes(out)


def tag(number: int, kind: int) -> bytes:
    return varint(number << 3 | kind)


def wire_type(field: wire.Field) -> int:
    return wire.WIRE_TYPES.get(field.kind, 0)


def packs(field: wire.Field) -> bool:
    return wire_type(field) != 2


def scalar(rng: random.Random, field: wire.Field) -> tuple[bytes, bool]:
    """scalar returns one value of a field that is no message, without its
    tag, and whether it is the zero value the receiver assumes."""
    kind = field.kind
    if kind == "enum":
        numbers = [number for number in (field.enum or {}) if isinstance(number, int)]
        number = rng.choice([0] + numbers)
        return varint(number), number == 0
    if kind == "bool":
        value = rng.random() < 0.6
        return (b"\1" if value else b"\0"), not value
    if kind in ("float", "double"):
        value = rng.choice(FLOATS)
        return struct.pack("<f" if kind == "float" else "<d", value), value == 0
    if kind == "fixed32":
        value = rng.choice([0, 1, 0xFFD34DFF, 0xFFFFFFFF])
        return struct.pack("<I", value), value == 0
    if kind == "fixed64":
        value = rng.choice([0, 1, 0x0110000100000001])
        return struct.pack("<Q", value), value == 0
    if kind == "string":
        value = rng.choice(TEXTS).encode()
        return varint(len(value)) + value, not value
    if kind == "bytes":
        value = bytes(rng.randrange(256) for _ in range(rng.choice([0, 1, 5, 200])))
        return varint(len(value)) + value, not value
    if kind == "int32" and (field.name == "player" or field.name.endswith("_player")):
        # A slot, or the -1 that stands for nobody, which the Rust library
        # holds as None and so writes back as -1 alone.
        value = rng.choice([0, 1, 7, 63, -1])
        return varint(value), value == 0
    limits = {"int32": (-(1 << 31), (1 << 31) - 1), "uint32": (0, (1 << 32) - 1), "int64": (-(1 << 63), (1 << 63) - 1), "uint64": (0, (1 << 64) - 1)}
    low, high = limits[kind]
    value = rng.choice([0, 1, 7, 127, 128, 300, high, low, rng.randint(low, high)])
    return varint(value), value == 0


def one(rng: random.Random, field: wire.Field, depth: int) -> tuple[bytes, bool]:
    """one returns one value of field without its tag, and whether the
    receiver assumes it."""
    if field.kind != "message":
        return scalar(rng, field)
    body = message(rng, field.message or "", depth + 1)
    return varint(len(body)) + body, False


def message(rng: random.Random, name: str, depth: int) -> bytes:
    """message returns one encoding of name as the Rust library writes it:
    the fields in schema order, without the zeros and empty lists the receiver
    assumes, and each list of numbers packed."""
    fields = SCHEMA[name][1]
    if name in UNIONS:
        field = rng.choice(fields)
        return tag(field.number, wire_type(field)) + one(rng, field, depth)[0]

    # Choose the case of each oneof, or none.
    cases: dict[str, wire.Field | None] = {}
    for field in fields:
        if field.oneof is not None:
            cases.setdefault(field.oneof, None)
    for oneof in cases:
        if rng.random() < 0.8:
            cases[oneof] = rng.choice([f for f in fields if f.oneof == oneof])

    boxed = holds_itself(name)
    deep = depth >= DEPTH
    out = bytearray()
    for field in fields:
        kind = wire_type(field)
        union = field.message in UNIONS
        if field.oneof is not None:
            if cases[field.oneof] is field:
                out += tag(field.number, kind) + one(rng, field, depth)[0]
        elif field.repeated:
            count = 0 if deep else rng.choice([0, 0, 1, 2, 3])
            values = [one(rng, field, depth)[0] for _ in range(count)]
            if packs(field) and values:
                body = b"".join(values)
                out += tag(field.number, 2) + varint(len(body)) + body
            elif not packs(field):
                for value in values:
                    out += tag(field.number, kind) + value
        elif field.optional or union or field.number in boxed:
            if not deep and rng.random() < 0.6:
                out += tag(field.number, kind) + one(rng, field, depth)[0]
        elif field.kind == "message":
            # The Rust library leaves out a message that sets nothing.
            body = message(rng, field.message or "", depth + 1)
            if body:
                out += tag(field.number, kind) + varint(len(body)) + body
        else:
            value, zero = one(rng, field, depth)
            if not zero:
                out += tag(field.number, kind) + value
    return bytes(out)


def main() -> None:
    rng = random.Random(20261121)
    lines = []
    for name in NAMES:
        if name not in SCHEMA:
            raise SystemExit(f"the Python library has no {name}")
        seen = set()
        for _ in range(SAMPLES):
            data = message(rng, name, 0)
            if data in seen:
                continue
            seen.add(data)
            wire.decode(SCHEMA, name, data)
            lines.append(f"{name} {data.hex()}\n")
    OUT.write_text("".join(lines), encoding="utf-8", newline="\n")
    print(f"{len(lines)} encodings of {len(NAMES)} messages")


main()
