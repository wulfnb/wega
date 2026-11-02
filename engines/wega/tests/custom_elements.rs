use std::sync::Arc;
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

macro_rules! assert_json_eq {
    ($actual:expr, $expected:expr $(,)?) => {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&$actual).unwrap(),
            serde_json::from_str::<serde_json::Value>($expected).unwrap()
        );
    };
}

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

    assert_json_eq!(
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

    assert_json_eq!(
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

    assert_json_eq!(
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

    assert_json_eq!(result, r#"{"closedCheck":true,"openCheck":true}"#);
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

    assert_json_eq!(
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

    assert_json_eq!(
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
          for (const invalidName of ["X-widget", "1-bad", "font-face", "missing-glyph", "nohyphen"]) {
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

    assert_json_eq!(
        result,
        r#"["new-html-element:TypeError","invalid-name-X-widget:SyntaxError","invalid-name-1-bad:SyntaxError","invalid-name-font-face:SyntaxError","invalid-name-missing-glyph:SyntaxError","invalid-name-nohyphen:SyntaxError","dup-name:NotSupportedError","dup-ctor:NotSupportedError","disallowed-host:NotSupportedError","dup-shadow:NotSupportedError","invalid-mode:TypeError"]"#
    );
}

#[test]
fn disconnected_upgrade_failure_and_definition_callbacks_are_stable() {
    let mut view = page(Limits::default());
    view.set_html("<div id=container></div>").unwrap();
    assert_json_eq!(view.evaluate_json(r#"(() => {
        const log = []; let reads = 0; let failures = 0;
        const detached = document.createElement('stable-element');
        class Stable extends HTMLElement {
            static get observedAttributes() { reads++; return ['data-value', 'style']; }
            constructor() { super(); this.constructed = true; }
            attributeChangedCallback(name, old, value, ns) { log.push([name, old, value, ns]); }
            connectedCallback() { log.push('connected'); }
            disconnectedCallback() { log.push('disconnected'); }
        }
        detached.setAttribute('data-value', 'initial');
        customElements.define('stable-element', Stable);
        const notUpgradedYet = !detached.constructed;
        customElements.upgrade(detached);
        Stable.prototype.attributeChangedCallback = () => log.push('wrong');
        detached.setAttribute('data-value', 'changed');
        document.body.append(detached);
        document.getElementById('container').append(detached);
        detached.remove();
        class Failed extends HTMLElement { constructor() { super(); failures++; throw Error('expected failure'); } }
        customElements.define('failed-element', Failed);
        const failed = document.createElement('failed-element');
        customElements.upgrade(failed); customElements.upgrade(failed);
        return {notUpgradedYet, upgraded: detached.constructed, reads, failures, log};
    })()"#).unwrap(), r#"{"failures":1,"log":[["data-value",null,"initial",null],["data-value","initial","changed",null],"connected","disconnected","connected","disconnected"],"notUpgradedYet":true,"reads":1,"upgraded":true}"#);
}

#[test]
fn invalid_registration_does_not_read_user_callbacks_and_closed_paths_hide_internals() {
    let mut view = page(Limits::default());
    view.set_html("<div id=host></div>").unwrap();
    assert_json_eq!(view.evaluate_json(r#"(() => {
        let reads = 0;
        class Bad extends HTMLElement { static get observedAttributes() { reads++; throw Error('must not run'); } }
        let invalid = false;
        try { customElements.define('X-widget', Bad); } catch(error) { invalid = error.name === 'SyntaxError'; }
        customElements.define('no-hyphen', class extends HTMLElement {});
        const host = document.getElementById('host');
        const root = host.attachShadow({mode: 'closed'});
        root.innerHTML = '<button id=inside>Inside</button>';
        const button = root.getElementById('inside');
        let inner = false, outer = false, escaped = false, cycles = false;
        button.addEventListener('test', event => { inner = event.composedPath().includes(root) && event.target === button; });
        host.addEventListener('test', event => { outer = event.target === host && !event.composedPath().includes(root) && !event.composedPath().includes(button); });
        host.addEventListener('private', () => { escaped = true; });
        button.dispatchEvent(new Event('test', {bubbles: true, composed: true}));
        button.dispatchEvent(new Event('private', {bubbles: true}));
        try { root.append(host); } catch(error) { cycles = /HierarchyRequestError/.test(error.name + error.message); }
        return {invalid, reads, inner, outer, escaped, cycles, hidden: host.shadowRoot === null && host._shadowRoot === undefined};
    })()"#).unwrap(), r#"{"cycles":true,"escaped":false,"hidden":true,"inner":true,"invalid":true,"outer":true,"reads":0}"#);
}
