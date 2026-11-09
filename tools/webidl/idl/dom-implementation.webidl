// Wega contributors, 2026. Simplified DOM/HTML binding surface; algorithms in semantic adapters.
// Simplified from vendor/servo-script-bindings/webidls/DOMImplementation.webidl.
// Stripped: [Throws], [NewObject], [LegacyNullToEmptyString], XMLDocument type.
// Note: createDocument returns Document (not XMLDocument) since XMLDocument is not yet generated.

[Exposed=Window] interface DOMImplementation {
 DocumentType createDocumentType(DOMString qualifiedName, DOMString publicId, DOMString systemId);
 Document createDocument(DOMString? namespace, DOMString qualifiedName, optional Document? doctype = null);
 Document createHTMLDocument(optional DOMString title);
 boolean hasFeature();
};
