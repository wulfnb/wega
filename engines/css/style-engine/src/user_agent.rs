//! The user-agent stylesheet.
use wega_css_parser::*;

pub fn default_user_agent_stylesheet() -> StyleSheet {
    StyleSheet::parse(
        r#"
head, style, script, meta, link, title, template, base, area, datalist, noscript, param, rp, source, track, dialog:not([open]) { display: none; }
[hidden]:not([hidden="until-found" i]) {display:none;}
html { display: block; }
body { display: block; margin: 8px; font-size: 16px; line-height: normal; color: black; }
address, blockquote, center, div, figure, figcaption, footer, form, header, hr, legend, listing, main, p, plaintext, pre, search, xmp,
article, aside, h1, h2, h3, h4, h5, h6, hgroup, nav, section, dialog, details, summary, fieldset, optgroup, menu, dir, dd, dl, dt, ol, ul, picture-caption { display: block; }
span, a, b, strong, i, em, u, s, small, big, sub, sup, abbr, cite, code, kbd, samp, var, mark, q, time, label, font, bdi, bdo, data, output, del, ins { display: inline; }
li { display: list-item; }
summary { display: block; }
a[href] { color: #0000ee; text-decoration: underline; }
u, ins { text-decoration: underline; }
s, del, strike { text-decoration: line-through; }
table { display: table; border-collapse: separate; border-spacing: 2px; box-sizing: border-box; }
caption { display: table-caption; text-align: center; }
colgroup { display: table-column-group; }
col { display: table-column; }
thead { display: table-header-group; }
tbody { display: table-row-group; }
tfoot { display: table-footer-group; }
tr { display: table-row; }
td, th { display: table-cell; padding: 1px; }
th { font-weight: bold; text-align: center; }
img, video, canvas, svg, iframe, embed, object { display: inline-block; }
button, input, textarea, select, meter, progress { display: inline-block; }
input, textarea, select, button { font-size: 13.333px; font-family: sans-serif; line-height: normal; color: black; }
input, textarea { box-sizing: content-box; width: 147px; padding: 1px 2px; border: 2px inset #767676; background-color: white; }
textarea { padding: 2px; border: 1px solid #767676; }
select { box-sizing: border-box; height: 20px; padding: 0 2px; border: 1px solid #767676; background-color: white; }
button, input[type="submit" i], input[type="button" i], input[type="reset" i] { box-sizing: border-box; width: auto; padding: 1px 6px; border: 1px solid #767676; border-radius: 2px; background-color: #efefef; text-align: center; }
input[type="checkbox" i], input[type="radio" i] { box-sizing: border-box; width: 13px; height: 13px; margin: 3px 3px 3px 4px; padding: 0; border: 0; background-color: transparent; }
input[type="hidden" i] { display: none; }
textarea { white-space: pre-wrap; width: 180px; height: 32px; }
iframe { border: 2px inset #767676; width: 300px; height: 150px; }
dialog[open] { position: absolute; left: 0; right: 0; margin: auto; border: 1.5px solid black; padding: 1em; background-color: white; color: black; width: fit-content; }
hr { margin: 8px 0; border: 1px inset #9a9a9a; height: 0; }
fieldset { margin: 0 2px; padding: 6px 12px 10px; border: 2px groove #c0c0c0; }
h1 { margin: 21.44px 0; font-size: 32px; font-weight: bold; }
h2 { margin: 19.92px 0; font-size: 24px; font-weight: bold; }
h3 { margin: 18.72px 0; font-size: 18.72px; font-weight: bold; }
h4 { margin: 21.28px 0; font-weight: bold; }
h5 { margin: 22.18px 0; font-size: 13.28px; font-weight: bold; }
h6 { margin: 24.97px 0; font-size: 10.72px; font-weight: bold; }
p, blockquote, figure, dl { margin: 16px 0; }
blockquote, figure { margin-left: 40px; margin-right: 40px; }
dd { margin-left: 40px; }
ul, ol, menu, dir { margin: 16px 0; padding-left: 40px; }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0; }
b, strong, dt { font-weight: bold; }
i, em, cite, var, dfn, address { font-style: italic; }
small { font-size: 0.83em; }
big { font-size: 1.17em; }
sub { vertical-align: sub; font-size: 0.83em; }
sup { vertical-align: super; font-size: 0.83em; }
mark { background-color: yellow; color: black; }
pre, xmp, listing, plaintext { white-space: pre; font-family: monospace; margin: 13px 0; }
code, kbd, samp, tt { font-family: monospace; }
center { text-align: center; }
center > table, center > :not(table, tbody, tr, td) > table, :not(td, th, tr, table)[align="center" i] > table { margin-left: auto; margin-right: auto; text-align: left; }
"#,
    )
}

// ---------------------------------------------------------------------------
// CASCADE & STYLE RESOLUTION
// ---------------------------------------------------------------------------

thread_local! {
    pub(crate) static USER_AGENT: StyleSheet = default_user_agent_stylesheet();
}
