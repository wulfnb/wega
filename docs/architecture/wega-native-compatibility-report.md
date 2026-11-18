# Native compatibility campaign

Production remains partly Blitz-backed; web standards support remains partial.
The severe homepage section overlap and rustdoc offscreen-main causes were
reduced against Chrome. This report distinguishes those fixes from visual parity.

## Deterministic reductions

Nine new fixtures cover indefinite column bases, scrollable sidebar minima,
preferred-size minimum caps, variable flex shorthand cascade, root rem sizing,
inline-block intrinsic width/baseline, padded flex basis/intrinsic sizing and
nested inline replaced elements and constrained-auto-width block centering. The existing mandatory Chrome/WPT runner now
includes them. Both browsers receive the identical original HTML including the
doctype, 600x420 viewport, scale 1 and Arial fixture styling. Assertions retain
channel tolerance 8, differing-pixel fraction 0.01 and geometry error 0.5px.
A missing Chrome fails the mandatory runner.

Before fixes the reduced column lost 180px of height, constrained-auto-width
centering missed by 150px, and sidebar fixtures displaced
main content by 1612.1875px. Those failures are archived as failure evidence,
not a successful baseline. The final comparison JSONs contain measured pixels
and geometry. No expected screenshot or legacy assertion was rewritten.

## Public sites

All four requested sites are navigated through the authorized broker. Wega and
Chrome captures use 800x600, scale 1, scroll 0 and light color preference. Chrome
waits for load, fonts.ready and two animation frames; Wega uses its original
bounded navigation/poll state. Equal font files/loading completion cannot be
asserted for public origins. These are fresh observations, not deterministic
pixel-parity gates or certification of the changing sites.

Baseline observations at the starting implementation:

| Site | Confirmed initial failure |
|---|---|
| rust-lang.org | `main` height 0; multiple sections at the same vertical origin and footer underneath the title |
| doc.rust-lang.org/std | Sidebar about 1957.7px wide; main x about 1957.7px and only 60px wide, outside the 800px viewport |
| example.com | Readable baseline; not a conformance claim |
| CERN original project | Readable baseline; not a conformance claim |

Final capture review and exact geometry are recorded in the campaign's
`public-review.json` and `public/public-compatibility.json` artifacts. Public pages
retain PARTIAL status; successful navigation and DOM text alone are insufficient.
Public-origin authentication/form submission and exhaustive interactions were
not tested. The local mandatory suites exercise real input/editing/focus,
checkboxes/radios, CSSOM insert/delete/disable, DOM mutations, fonts, frame
creation/removal/origin access, pointer/keyboard routing, scrolling and native
accessibility. Their DOM host still uses the production Blitz owner.

## Unsupported and partial features

The targeted property audit still finds unsupported native semantics for
`align-content`, `align-self`, `float`, `clear`, `order`, `inset`,
`grid-template-areas`, `grid-area`, `overflow-wrap`, `word-break`, `text-overflow`,
`text-decoration`, `border-collapse`, `list-style`, `margin-inline-start` and
`contain`. The public example page also uses unsupported `light-dark()` color
selection. No no-op property implementation was added to remove diagnostics.
Basic existing flex/grid/table, physical margins, overflow, fonts and at-rules
remain bounded subsets. Additional selector/media grammar, logical writing-mode
layout, advanced typography, decorations/list markers, full SVG/Canvas/media,
complete AccName/ARIA and cross-frame accessibility remain partial or unsupported.
Native getComputedStyle serialization also exposes only its existing bounded
property set; unsupported values must not be interpreted as Chrome equivalence.

Legacy reference-renderer differences are retained in the visual corpus report.
Where Chrome supports native geometry, a reference mismatch is recorded as such;
we do not change native output to match known incorrect reference geometry.
Pinned WPT coverage is the selected DOM subset and existing overflow/frame
fixtures, not the full WPT suite. Exact final counts are in the removal evidence.

Additional observed gaps in the paired captures include UA/default serif fonts and
definition-list indentation on CERN, and font/line-height/color-function differences
on example.com. Raw unsupported-property diagnostics are not a complete feature
detector: deferred shorthands can still be listed despite the reduced supported
case, while unsupported values of a known property need separate visual evaluation.

Computed-style serialization has correctness gaps beyond missing property names:
its existing width/height serialization uses border-box dimensions. For example,
the example.com body rectangle is 480px wide while Chrome's content-box CSS width
is 416px. This was not corrected or represented as computed-style equivalence.

## Final reviewed captures

| Page | Native | Chrome | Result |
|---|---|---|---|
| example | [capture](baseline/blitz-removal-2026-11-17/public/public-example-native.png) | [capture](baseline/blitz-removal-2026-11-17/public-chrome/public-example-chrome.png) | PARTIAL; Constrained auto-width body now centers at x=160 with 480px border-box width, matching Chrome geometry. Background light-dark() selection, font metrics and line breaks remain different. |
| cern | [capture](baseline/blitz-removal-2026-11-17/public/public-cern-native.png) | [capture](baseline/blitz-removal-2026-11-17/public-chrome/public-cern-chrome.png) | PARTIAL; Readable main text and links with coherent flow. Default serif font, heading metrics, underlines and definition-list indentation differ from Chrome. |
| rust | [capture](baseline/blitz-removal-2026-11-17/public/public-rust-native.png) | [capture](baseline/blitz-removal-2026-11-17/public-chrome/public-rust-chrome.png) | PARTIAL; Navigation/logo and title are separated; main has nonzero height and sections/footer no longer share the collapsed origin. Remaining differences include text-decoration, text-transform/button appearance, vertical spacing and later-page/document-height discrepancies. |
| rust-std | [capture](baseline/blitz-removal-2026-11-17/public/public-rust-std-native.png) | [capture](baseline/blitz-removal-2026-11-17/public-chrome/public-rust-std-chrome.png) | PARTIAL; Sidebar is 200px; main starts at x=200 with width 600, and the 540px content column is readable. Toolbar/icon/logo placement, serif body fonts, truncation and overall height still differ substantially. |

[Manual review and exact paired geometry](baseline/blitz-removal-2026-11-17/public-review.json)
and [all 25 strict Chrome comparisons](baseline/blitz-removal-2026-11-17/strict-chrome-summary.json)
are archived. The legacy native/reference corpus remains 8/12 joint gates; login,
scrolling, table and transformed overlap differ from the reference. The pinned
Chrome gates still pass, so reference differences are not silently converted into
native regressions or removed tests.
