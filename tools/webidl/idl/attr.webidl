// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface Attr : Node {
 readonly attribute DOMString? namespaceURI;
 readonly attribute DOMString? prefix;
 readonly attribute DOMString localName;
 readonly attribute DOMString name;
 readonly attribute boolean specified;
 attribute DOMString value;
 readonly attribute Element? ownerElement;
};
