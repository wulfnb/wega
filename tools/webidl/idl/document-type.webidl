// Wega contributors, 2026. Simplified DOM/HTML binding surface; algorithms in semantic adapters.
// Simplified from vendor/servo-script-bindings/webidls/DocumentType.webidl.
// Simplification rules: [Constant] → [Constant] kept, interface-typed params kept.

[Exposed=Window] interface DocumentType : Node {
 readonly attribute DOMString name;
 readonly attribute DOMString publicId;
 readonly attribute DOMString systemId;
};

DocumentType includes ChildNode;
