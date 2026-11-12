# html/parser — `wega-html`

**Status: IMPLEMENTED (Round 7)** — the incremental HTML parser and fragment parser.

`Parser` (pauses after each parser-inserted `</script>`, accepts `document.write` at the insertion point), `Step`, `parse_fragment` (innerHTML, `insertAdjacentHTML`, contextual fragments, template contents) and the tree sink that builds through `wega_dom::Mutator`. Terminates at the node budget without a placeholder handle, never runs a script after termination. Warnings are reported through the parser-owned `HtmlDiagnostic`/`ParseReport` boundary.

## Dependencies

`wega-dom`, `html5ever`, `url`. No engine, no Blitz, no diagnostics module. XML parsing is **not** here (it stays in the engine until it has an independent boundary). Tests that need a real document live in `wega-adapter-blitz` (`tests/html_parser.rs`). `tests/boundary.rs` enforces the dependency list.
