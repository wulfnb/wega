// Wega contributors, 2026. Simplified DOM/HTML binding surface; algorithms in semantic adapters.
// Simplified from vendor/servo-script-bindings/webidls/ProcessingInstruction.webidl.
// Simplification rules: [Constant] → kept.

[Exposed=Window] interface ProcessingInstruction : CharacterData {
 readonly attribute DOMString target;
};
