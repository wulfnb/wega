// Wega contributors, 2026. Simplified DOM/HTML binding surface; algorithms in semantic adapters.
// Simplified from vendor/servo-script-bindings/webidls/Attr.webidl.
// Stripped: [Constant], [CEReactions], [Pure], [SetterThrows], optional defaults.
// Attr.namespaceURI is namespace-aware (may be null for non-namespace attributes).

[Exposed=Window] interface Attr : Node {
 readonly attribute DOMString? namespaceURI;
 readonly attribute DOMString? prefix;
 readonly attribute DOMString localName;
 readonly attribute DOMString name;
 readonly attribute boolean specified;
 attribute DOMString value;
 readonly attribute Element? ownerElement;
};
