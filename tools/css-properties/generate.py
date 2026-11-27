#!/usr/bin/env python3
"""Generate Wega's CSS property table from Stylo's property metadata.

Reads the pinned, vendored Stylo 0.22.0 `properties/{longhands,shorthands}.toml`
(MPL-2.0) and writes `engines/css/parser/src/generated/properties.rs`: every property the
Servo engine build supports (with Wega's enabled preferences), its value type, keyword
values, inheritance, aliases and, for shorthands, longhands and serialization kind.

Run from the repository root:  python3 tools/css-properties/generate.py
"""
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STYLO = ROOT / "engines/vendor/blitz/stylo/properties"
SPECIFIED = ROOT / "engines/vendor/blitz/stylo/values/specified"
# Generic value types shared by specified and computed values (`TransformStyle`, `FillRule`).
GENERICS = ROOT / "engines/vendor/blitz/stylo/values/generics"
OUT = ROOT / "engines/css/parser/src/generated/properties.rs"

# Style structs whose properties inherit (Stylo `properties/data.py`, `StyleStruct`).
INHERITED_STRUCTS = {
    "font", "inherited_box", "inherited_svg", "inherited_table", "inherited_text",
    "inherited_ui", "list",
}
# Properties Stylo gates behind a preference Wega leaves off, but the native engine
# implements.
NATIVE_EXTRAS = {"writing-mode"}
# Stylo preferences Wega enables (`engines/wega/src/document/mod.rs`).
ENABLED_PREFS = {
    "layout.grid.enabled", "layout.flexbox.balance", "layout.unimplemented",
    "layout.columns.enabled", "layout.css.basic-shape-shape.enabled",
    "layout.css.tree-counting-functions.enabled", "layout.css.progress-function.enabled",
    "layout.variable_fonts.enabled", "layout.container-queries.enabled",
}


def servo(entry):
    """Supported by the Servo build and exposed to web content (not UA- or chrome-only)."""
    return entry.get("engine", "both") in ("both", "servo") and entry.get(
        "enabled_in", "content"
    ) not in ("ua", "chrome")


def aliases(name, entry):
    out = list(entry.get("aliases") or [])
    for prefix in entry.get("extra_prefixes") or []:
        if ":" in prefix:  # Gecko-only, behind a Gecko preference
            continue
        out.append(f"-{prefix}-{name}")
    return out


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rust_list(items):
    return "&[" + ", ".join(rust_str(i) for i in items) + "]"


def css_identifier(variant):
    """Stylo's derive naming (`style_derive::to_css`): CamelCase to kebab-case, with a
    `Moz`/`Webkit` prefix becoming `-moz-`/`-webkit-`."""
    out = re.sub(r"(?<!^)([A-Z])", lambda m: "-" + m.group(1), variant).lower()
    for vendor in ("moz", "webkit"):
        if out.startswith(vendor + "-"):
            out = "-" + out
    return out


def derived_enums(text):
    """`(derives, attributes, name, body)` for each `#[derive(...)]` followed (after attribute
    and doc-comment lines) by `pub enum Name {`; the body ends at the first line that is `}`.
    A scan with `str.find`, not a regex: nested repetition there backtracks exponentially."""
    out, pos = [], 0
    while (start := text.find("#[derive(", pos)) >= 0:
        pos = start + 1
        close = text.find(")]", start)
        if close < 0:
            break
        derives = text[start + len("#[derive(") : close]
        if "]" in derives:
            continue
        i, attrs = close + 2, []
        while True:
            j = i
            while j < len(text) and text[j].isspace():
                j += 1
            if text.startswith("#[", j) or text.startswith("///", j):
                end = text.find("\n", j)
                end = len(text) if end < 0 else end
                line = text[j:end]
                if line.startswith("#[") and not line.rstrip().endswith("]"):
                    break
                attrs.append(text[i:end])
                i = end
            else:
                i = j
                break
        m = re.compile(r"pub enum (\w+)\s*\{").match(text, i)
        if not m:
            continue
        end = text.find("\n}", m.end())
        if end < 0:
            continue
        out.append((derives, "".join(attrs), m.group(1), text[m.end() : end]))
    return out


def keyword_enums():
    """Specified and generic value types that are `#[derive(Parse)]` enums of unit variants only:
    `{type: [(keyword, [aliases])]}`. Variants with a parse condition are left out."""
    found = {}
    # Specified types first: a generic with the same name does not replace one.
    for path in [*sorted(SPECIFIED.rglob("*.rs")), *sorted(GENERICS.rglob("*.rs"))]:
        for derives, attrs, name, body in derived_enums(path.read_text()):
            if "Parse" not in derives or name in found:
                continue
            variants, ok = [], True
            pending = []
            for line in body.splitlines():
                line = line.strip()
                if not line or line.startswith("//"):
                    continue
                if line.startswith("#["):
                    pending.append(line)
                    continue
                m = re.fullmatch(r"(\w+)(\s*=\s*[^,]+)?,?", line)
                if not m:
                    ok = False
                    break
                joined = " ".join(pending)
                pending = []
                # Variants behind a parse condition (chrome-only rules, or preferences that are
                # off for author style) are not keywords of the value.
                if "css(skip)" in joined or "condition" in joined:
                    continue
                keyword = re.search(r'keyword\s*=\s*"([^"]+)"', joined)
                aliases = re.search(r'aliases\s*=\s*"([^"]+)"', joined)
                css_name = keyword.group(1) if keyword else css_identifier(m.group(1))
                # Gecko-only variants are kept when they are standard CSS (`pointer-events:
                # visiblePainted`, `word-break: break-word`), not when Gecko-internal (`-moz-`).
                if 'feature = "gecko"' in joined and css_name.startswith("-moz-"):
                    continue
                variants.append((
                    css_name,
                    aliases.group(1).split("|") if aliases else [],
                ))
            if ok and variants:
                found[name] = variants
    # Hand-written `impl Parse` that only matches identifiers to variants
    # (`try_match_ident_ignore_ascii_case!`); any other logic leaves the type to Stylo.
    impl = re.compile(
        r"impl Parse for (\w+) \{\s*fn parse\([^)]*\)\s*->\s*Result<Self, ParseError[^>]*>\s*\{"
        r"\s*Ok\(try_match_ident_ignore_ascii_case! \{ input,(.*?)\}\)\s*\}\s*\}",
        re.S,
    )
    arm = re.compile(r'((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>\s*Self::(\w+),?')
    for path in sorted(SPECIFIED.rglob("*.rs")):
        for name, body in impl.findall(path.read_text()):
            if name in found:
                continue
            # Arms behind a preference Wega leaves off never match.
            body = re.sub(
                r'"[^"]+"\s+if crate::pref!\("([^"]+)"\)\s*=>\s*\{[^{}]*\},?',
                lambda m: "" if m.group(1) not in ENABLED_PREFS else m.group(0),
                body,
            )
            arms = [a.strip() for a in body.strip().splitlines() if a.strip() and not a.strip().startswith("//")]
            parsed = [arm.fullmatch(a) for a in arms]
            if not parsed or not all(parsed):
                continue
            by_variant = {}
            for m in parsed:
                words = re.findall(r'"([^"]+)"', m.group(1))
                by_variant.setdefault(m.group(2), []).extend(words)
            variants = []
            for variant, words in by_variant.items():
                keyword = css_identifier(variant)
                if keyword not in words:
                    break
                variants.append((keyword, [w for w in words if w != keyword]))
            else:
                found[name] = variants
    return found


def main():
    longhands = tomllib.loads((STYLO / "longhands.toml").read_text())
    shorthands = tomllib.loads((STYLO / "shorthands.toml").read_text())
    lines = [
        "//! CSS property table, generated by `tools/css-properties/generate.py` from Stylo",
        "//! 0.22.0 `properties/{longhands,shorthands}.toml` (MPL-2.0). Do not edit by hand.",
        "use super::super::properties_table::{KeywordType, Longhand, Shorthand};",
        "",
        "/// Longhands supported by the Servo build of Stylo, by name.",
        "pub static LONGHANDS: &[Longhand] = &[",
    ]
    alias_pairs = []
    for name in sorted(longhands):
        e = longhands[name]
        if not (servo(e) or name in NATIVE_EXTRAS):
            continue
        keywords = (e.get("keyword") or {}).get("values") or []
        value_type = e.get("type") or ("keyword" if keywords else "")
        lines.append(
            f"    Longhand {{ name: {rust_str(name)}, value_type: {rust_str(value_type)}, "
            f"keywords: {rust_list(keywords)}, inherited: {str(e.get('struct') in INHERITED_STRUCTS).lower()}, "
            f"logical: {str(bool(e.get('logical'))).lower()}, list: {str('vector' in e).lower()}, "
            f"logical_group: {rust_str(e.get('logical_group') or '')}, "
            f"non_negative: {str('non_negative' in (e.get('parse_method') or '')).lower()}, "
            f"parse_method: {rust_str(e.get('parse_method') or '')} }},"
        )
        alias_pairs += [(a, name) for a in aliases(name, e)]
    lines += ["];", "", "/// Shorthands supported by the Servo build of Stylo, by name.",
              "pub static SHORTHANDS: &[Shorthand] = &["]
    # `all` covers every longhand but `direction` and `unicode-bidi` (CSS Cascade 4 §3.1).
    shorthands = dict(shorthands)
    shorthands["all"] = {
        "sub_properties": [
            n for n in sorted(longhands)
            if (servo(longhands[n]) or n in NATIVE_EXTRAS) and n not in ("direction", "unicode-bidi")
        ],
        "kind": "all",
    }
    for name in sorted(shorthands):
        e = shorthands[name]
        if not servo(e):
            continue
        lines.append(
            f"    Shorthand {{ name: {rust_str(name)}, longhands: {rust_list(e['sub_properties'])}, "
            f"kind: {rust_str(e.get('kind', ''))} }},"
        )
        alias_pairs += [(a, name) for a in aliases(name, e)]
    lines += ["];", "", "/// Legacy and prefixed aliases: (alias, property), by alias.",
              "pub static ALIASES: &[(&str, &str)] = &["]
    for alias, name in sorted(alias_pairs):
        lines.append(f"    ({rust_str(alias)}, {rust_str(name)}),")
    lines.append("];")
    # Keyword enum value types used by the properties above.
    enums = keyword_enums()
    used = sorted({longhands[n].get("type", "").split("::")[-1] for n in longhands} & set(enums))
    lines += ["", "/// Value types that are enums of keywords: (type, [(keyword, aliases)]), by type.",
              "pub static KEYWORD_TYPES: &[KeywordType] = &["]
    for name in used:
        entries = ", ".join(f"({rust_str(k)}, {rust_list(a)})" for k, a in enums[name])
        lines.append(f"    ({rust_str(name)}, &[{entries}]),")
    lines.append("];")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {sum(l.startswith('    Longhand') for l in lines)} longhands, "
          f"{sum(l.startswith('    Shorthand') for l in lines)} shorthands, {len(alias_pairs)} aliases")


if __name__ == "__main__":
    main()
