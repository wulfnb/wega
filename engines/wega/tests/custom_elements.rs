use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

fn page(limits: Limits) -> wega_engine::View {
    Engine::new(limits, NetworkPolicy::default(), Arc::new(|| {}))
        .unwrap()
        .create_view(Url::parse("https://example.test/index.html").unwrap())
        .unwrap()
}

#[test]
fn custom_element_definition_and_instantiation() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Custom Elements</title>
      <my-counter id="parsed" count="5"></my-counter>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          class MyCounter extends HTMLElement {
            constructor() {
              super();
              this.initialized = true;
            }
          }
          customElements.define("my-counter", MyCounter);

          // 1. Definition verification
          const getCheck = customElements.get("my-counter") === MyCounter;
          const nameCheck = customElements.getName(MyCounter) === "my-counter";

          // 2. Instantiation via `new`
          const inst = new MyCounter();
          const newCheck = inst instanceof MyCounter &&
                           inst instanceof HTMLElement &&
                           inst.tagName === "MY-COUNTER" &&
                           inst.initialized === true;

          // 3. Instantiation via createElement
          const created = document.createElement("my-counter");
          const createCheck = created instanceof MyCounter && created.initialized === true;

          // 4. Existing parsed element upgrade
          const parsed = document.getElementById("parsed");
          const parsedCheck = parsed instanceof MyCounter && parsed.initialized === true;

          return {
            getCheck,
            nameCheck,
            newCheck,
            createCheck,
            parsedCheck
          };
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"{"createCheck":true,"getCheck":true,"nameCheck":true,"newCheck":true,"parsedCheck":true}"#
    );
}

#[test]
fn custom_element_lifecycle_connected_disconnected_and_attribute_changed() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Custom Element Lifecycle</title>
      <div id="container"></div>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          const log = [];
          class LifecycleElement extends HTMLElement {
            static get observedAttributes() {
              return ["active", "theme"];
            }
            connectedCallback() {
              log.push("connected");
            }
            disconnectedCallback() {
              log.push("disconnected");
            }
            attributeChangedCallback(name, oldValue, newValue) {
              log.push(`attr:${name}:${oldValue}->${newValue}`);
            }
          }
          customElements.define("lifecycle-element", LifecycleElement);

          const el = document.createElement("lifecycle-element");
          el.setAttribute("active", "true");
          el.setAttribute("unobserved", "ignore");

          document.getElementById("container").appendChild(el);
          el.setAttribute("theme", "dark");
          el.removeAttribute("active");
          el.remove();

          return log;
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"["attr:active:null->true","connected","attr:theme:null->dark","attr:active:true->null","disconnected"]"#
    );
}

#[test]
fn custom_element_when_defined_and_late_upgrade() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Late Upgrade</title>
      <late-element id="early" prop="1"></late-element>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          let resolved = false;
          customElements.whenDefined("late-element").then((ctor) => {
            if (ctor) resolved = true;
          });

          const early = document.getElementById("early");
          const beforeCheck = !(early instanceof Function);

          class LateElement extends HTMLElement {
            static get observedAttributes() { return ["prop"]; }
            constructor() {
              super();
              this.upgraded = true;
            }
            attributeChangedCallback(name, oldVal, newVal) {
              this.receivedProp = newVal;
            }
          }

          customElements.define("late-element", LateElement);

          return {
            beforeCheck,
            afterCheck: early instanceof LateElement,
            upgraded: early.upgraded === true,
            receivedProp: early.receivedProp === "1"
          };
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"{"afterCheck":true,"beforeCheck":true,"receivedProp":true,"upgraded":true}"#
    );
}

#[test]
fn shadow_root_open_and_closed_encapsulation() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Shadow DOM Encapsulation</title>
      <div id="open-host"></div>
      <div id="closed-host"></div>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          const openHost = document.getElementById("open-host");
          const openRoot = openHost.attachShadow({ mode: "open" });
          openRoot.innerHTML = '<span id="inner-open">Hello Open</span>';

          const openCheck = openHost.shadowRoot === openRoot &&
                            openRoot.host === openHost &&
                            openRoot.mode === "open" &&
                            openRoot.getElementById("inner-open") !== null &&
                            document.getElementById("inner-open") === null;

          const closedHost = document.getElementById("closed-host");
          const closedRoot = closedHost.attachShadow({ mode: "closed" });
          closedRoot.innerHTML = '<span id="inner-closed">Hello Closed</span>';

          const closedCheck = closedHost.shadowRoot === null &&
                              closedRoot.host === closedHost &&
                              closedRoot.mode === "closed" &&
                              closedRoot.getElementById("inner-closed") !== null &&
                              document.getElementById("inner-closed") === null;

          return { openCheck, closedCheck };
        })()"#,
        )
        .unwrap();

    assert_eq!(result, r#"{"closedCheck":true,"openCheck":true}"#);
}

#[test]
fn shadow_root_slot_assignment_named_default_and_flatten() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Slot Assignment</title>
      <div id="host">
        <h1 slot="title" id="slotted-title">My Title</h1>
        <p id="slotted-body">My Body Paragraph</p>
      </div>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          const host = document.getElementById("host");
          const root = host.attachShadow({ mode: "open" });
          root.innerHTML = `
            <header><slot name="title" id="title-slot"></slot></header>
            <main><slot id="default-slot"></slot></main>
            <footer><slot name="footer" id="fallback-slot"><span>Default Footer</span></slot></footer>
          `;

          const titleSlot = root.getElementById("title-slot");
          const defaultSlot = root.getElementById("default-slot");
          const fallbackSlot = root.getElementById("fallback-slot");

          const slottedTitle = document.getElementById("slotted-title");
          const slottedBody = document.getElementById("slotted-body");

          const titleAssigned = titleSlot.assignedElements();
          const defaultAssigned = defaultSlot.assignedElements();
          const fallbackAssigned = fallbackSlot.assignedElements();
          const fallbackFlattened = fallbackSlot.assignedElements({ flatten: true });

          return {
            titleMatches: titleAssigned.length === 1 && titleAssigned[0] === slottedTitle,
            bodyMatches: defaultAssigned.length === 1 && defaultAssigned[0] === slottedBody,
            titleSlotBackRef: slottedTitle.assignedSlot === titleSlot,
            bodySlotBackRef: slottedBody.assignedSlot === defaultSlot,
            fallbackEmptyWithoutFlatten: fallbackAssigned.length === 0,
            fallbackHasContentWithFlatten: fallbackFlattened.length === 1 && fallbackFlattened[0].textContent === "Default Footer"
          };
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"{"bodyMatches":true,"bodySlotBackRef":true,"fallbackEmptyWithoutFlatten":true,"fallbackHasContentWithFlatten":true,"titleMatches":true,"titleSlotBackRef":true}"#
    );
}

#[test]
fn event_retargeting_and_composed_path_across_shadow_boundary() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Event Retargeting</title>
      <div id="container">
        <div id="host"></div>
      </div>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          const host = document.getElementById("host");
          const container = document.getElementById("container");
          const root = host.attachShadow({ mode: "open" });
          root.innerHTML = '<button id="btn">Click</button>';
          const btn = root.getElementById("btn");

          let innerTarget = null;
          let outerTarget = null;
          let containerTarget = null;

          btn.addEventListener("test-composed", (e) => {
            innerTarget = e.target.id;
          });

          host.addEventListener("test-composed", (e) => {
            outerTarget = e.target.id;
          });

          container.addEventListener("test-composed", (e) => {
            containerTarget = e.target.id;
          });

          const composedEv = new Event("test-composed", { bubbles: true, composed: true });
          btn.dispatchEvent(composedEv);

          // Now test non-composed event containment:
          let hostSawNonComposed = false;
          let innerSawNonComposed = false;

          btn.addEventListener("test-non-composed", (e) => {
            innerSawNonComposed = true;
          });

          host.addEventListener("test-non-composed", (e) => {
            hostSawNonComposed = true;
          });

          const nonComposedEv = new Event("test-non-composed", { bubbles: true, composed: false });
          btn.dispatchEvent(nonComposedEv);

          return {
            innerTarget,
            outerTarget,
            containerTarget,
            innerSawNonComposed,
            hostSawNonComposed
          };
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"{"containerTarget":"host","hostSawNonComposed":false,"innerSawNonComposed":true,"innerTarget":"btn","outerTarget":"host"}"#
    );
}

#[test]
fn security_and_spec_conformance_fail_closed() {
    let mut view = page(Limits::default());
    view.set_html(
        r#"<!doctype html>
      <title>Security and Fail-Closed Conformance</title>
      <input id="input-elem" />
      <div id="valid-host"></div>
    "#,
    )
    .unwrap();

    let result = view
        .evaluate_json(
            r#"(() => {
          const errors = [];

          // 1. Direct new HTMLElement() must fail
          try {
            new HTMLElement();
          } catch (e) {
            errors.push(`new-html-element:${e.name}`);
          }

          // 2. Invalid custom element names
          for (const invalidName of ["myElement", "1-bad", "font-face", "missing-glyph", "no-hyphen"]) {
            try {
              customElements.define(invalidName, class extends HTMLElement {});
            } catch (e) {
              errors.push(`invalid-name-${invalidName}:${e.name}`);
            }
          }

          // 3. Duplicate registration
          class CompA extends HTMLElement {}
          class CompB extends HTMLElement {}
          customElements.define("custom-comp", CompA);
          try {
            customElements.define("custom-comp", CompB);
          } catch (e) {
            errors.push(`dup-name:${e.name}`);
          }
          try {
            customElements.define("other-comp", CompA);
          } catch (e) {
            errors.push(`dup-ctor:${e.name}`);
          }

          // 4. Disallowed shadow hosts (<input>)
          const input = document.getElementById("input-elem");
          try {
            input.attachShadow({ mode: "open" });
          } catch (e) {
            errors.push(`disallowed-host:${e.name}`);
          }

          // 5. Duplicate attachShadow on same host
          const valid = document.getElementById("valid-host");
          valid.attachShadow({ mode: "open" });
          try {
            valid.attachShadow({ mode: "open" });
          } catch (e) {
            errors.push(`dup-shadow:${e.name}`);
          }

          // 6. Invalid attachShadow mode
          const other = document.createElement("div");
          try {
            other.attachShadow({ mode: "invalid" });
          } catch (e) {
            errors.push(`invalid-mode:${e.name}`);
          }

          return errors;
        })()"#,
        )
        .unwrap();

    assert_eq!(
        result,
        r#"["new-html-element:TypeError","invalid-name-myElement:SyntaxError","invalid-name-1-bad:SyntaxError","invalid-name-font-face:SyntaxError","invalid-name-missing-glyph:SyntaxError","invalid-name-no-hyphen:SyntaxError","dup-name:NotSupportedError","dup-ctor:NotSupportedError","disallowed-host:NotSupportedError","dup-shadow:NotSupportedError","invalid-mode:TypeError"]"#
    );
}

