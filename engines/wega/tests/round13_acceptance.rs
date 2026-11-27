//! Round 13 Acceptance Tests: HTML & DOM completeness, form validation, live collections,
//! and Shadow DOM/custom element lifecycle.
mod support;
use serde_json::Value;
use wega_engine::RenderingBackend;

#[test]
fn form_validation_validity_state_and_check_validity() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<form id="test-form">
  <input id="req-input" type="text" required />
  <input id="email-input" type="email" value="invalid-email" />
  <input id="pattern-input" type="text" pattern="[0-9]{3}" value="abc" />
  <input id="num-input" type="number" min="10" max="50" value="5" />
  <select id="req-select" required>
    <option value="">Choose...</option>
    <option value="opt1">Option 1</option>
  </select>
  <textarea id="req-textarea" required></textarea>
  <button id="submit-btn" type="submit">Submit</button>
</form>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/form"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const req = document.getElementById('req-input');
        const email = document.getElementById('email-input');
        const pattern = document.getElementById('pattern-input');
        const num = document.getElementById('num-input');
        const select = document.getElementById('req-select');
        const textarea = document.getElementById('req-textarea');

        return {
            reqMissing: req.validity.valueMissing,
            reqValid: req.checkValidity(),
            emailMismatch: email.validity.typeMismatch,
            patternMismatch: pattern.validity.patternMismatch,
            rangeUnderflow: num.validity.rangeUnderflow,
            selectMissing: select.validity.valueMissing,
            textareaMissing: textarea.validity.valueMissing
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["reqMissing"], true);
    assert_eq!(v["reqValid"], false);
    assert_eq!(v["emailMismatch"], true);
    assert_eq!(v["patternMismatch"], true);
    assert_eq!(v["rangeUnderflow"], true);
    assert_eq!(v["selectMissing"], true);
    assert_eq!(v["textareaMissing"], true);

    let res2 = view
        .evaluate_json(
            r#"(() => {
        const req = document.getElementById('req-input');
        req.value = "filled";
        const valAfterSet = req.value;
        const missingAfterSet = req.validity.valueMissing;

        req.setCustomValidity("Custom error");
        const customInvalid = req.validity.customError && !req.checkValidity();
        const customMsg = req.validationMessage;

        req.setCustomValidity("");
        const customAfterClear = req.validity.customError;
        const validAfterClear = req.validity.valid;
        const checkAfterClear = req.checkValidity();

        return {
            valAfterSet,
            missingAfterSet,
            customInvalid,
            customMsg,
            customAfterClear,
            validAfterClear,
            checkAfterClear,
            typeMismatch: req.validity.typeMismatch,
            patternMismatch: req.validity.patternMismatch,
            tooLong: req.validity.tooLong,
            tooShort: req.validity.tooShort,
            rangeUnderflow: req.validity.rangeUnderflow,
            rangeOverflow: req.validity.rangeOverflow,
            stepMismatch: req.validity.stepMismatch,
            badInput: req.validity.badInput,
            customError: req.validity.customError
        };
    })()"#,
        )
        .unwrap();

    let v2: Value = serde_json::from_str(&res2).unwrap();
    println!("DEBUG v2: {:?}", v2);
    assert_eq!(v2["valAfterSet"], "filled");
    assert_eq!(v2["missingAfterSet"], false);
    assert_eq!(v2["customInvalid"], true);
    assert_eq!(v2["customMsg"], "Custom error");
    assert_eq!(v2["customAfterClear"], false);
    assert_eq!(v2["validAfterClear"], true);
    assert_eq!(v2["checkAfterClear"], true);
}

#[test]
fn select_and_options_properties_and_live_collection() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<select id="single-sel">
  <option value="a">A</option>
  <option value="b" selected>B</option>
  <option value="c">C</option>
</select>
<select id="multi-sel" multiple>
  <option value="x" selected>X</option>
  <option value="y">Y</option>
  <option value="z" selected>Z</option>
</select>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/select"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const s = document.getElementById('single-sel');
        const m = document.getElementById('multi-sel');

        const initialSingleVal = s.value;
        const initialSingleIndex = s.selectedIndex;
        s.value = "c";

        const multiVals = [...m.selectedOptions].map(o => o.value);
        return {
            initialSingleVal,
            initialSingleIndex,
            newSingleVal: s.value,
            newSingleIndex: s.selectedIndex,
            multiLength: m.options.length,
            multiSelected: multiVals
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["initialSingleVal"], "b");
    assert_eq!(v["initialSingleIndex"], 1);
    assert_eq!(v["newSingleVal"], "c");
    assert_eq!(v["newSingleIndex"], 2);
    assert_eq!(v["multiLength"], 3);
    assert_eq!(v["multiSelected"], serde_json::json!(["x", "z"]));
}

#[test]
fn dom_tree_mutations_and_event_cancellation_cycle() {
    let server = support::Server::new(|_| {
        support::Reply::html(
            r#"<div id="container">
  <div id="child1">Child 1</div>
  <div id="child2">Child 2</div>
</div>"#,
        )
    });
    let engine = support::engine();
    let mut view = support::open(&engine, &server.url("localhost", "/dom"));
    view.set_rendering_backend(RenderingBackend::WegaNative);

    let res = view
        .evaluate_json(
            r#"(() => {
        const c = document.getElementById('container');
        const p = document.createElement('p');
        p.id = "p-inserted";
        p.textContent = "Inserted Paragraph";

        c.prepend(p);
        const firstIsP = c.firstElementChild.id === "p-inserted";

        let cancelled = false;
        c.addEventListener("custom-test", (e) => {
            if (e.cancelable) {
                e.preventDefault();
            }
        });
        const ev = new CustomEvent("custom-test", { bubbles: true, cancelable: true });
        p.dispatchEvent(ev);
        cancelled = ev.defaultPrevented;

        c.replaceChildren(p);
        const onlyP = c.children.length === 1 && c.children[0].id === "p-inserted";

        return {
            firstIsP,
            cancelled,
            onlyP
        };
    })()"#,
        )
        .unwrap();

    let v: Value = serde_json::from_str(&res).unwrap();
    assert_eq!(v["firstIsP"], true);
    assert_eq!(v["cancelled"], true);
    assert_eq!(v["onlyP"], true);
}
