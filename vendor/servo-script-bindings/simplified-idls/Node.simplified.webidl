/* Simplified pin for step 08.f: Node family generator pipeline.
 * Source: vendor/servo-script-bindings/webidls/Node.webidl
 * Divergence: simplified to match the generator's supported subset.
 * Full signatures (nullable interface types, dictionaries, interface-
 * typed parameters/returns) stay in the JS layer. */
[Exposed=Window, Abstract]
interface Node : EventTarget {
  const unsigned short ELEMENT_NODE = 1;
  const unsigned short ATTRIBUTE_NODE = 2;
  const unsigned short TEXT_NODE = 3;
  const unsigned short CDATA_SECTION_NODE = 4;
  const unsigned short ENTITY_REFERENCE_NODE = 5;
  const unsigned short ENTITY_NODE = 6;
  const unsigned short PROCESSING_INSTRUCTION_NODE = 7;
  const unsigned short COMMENT_NODE = 8;
  const unsigned short DOCUMENT_NODE = 9;
  const unsigned short DOCUMENT_TYPE_NODE = 10;
  const unsigned short DOCUMENT_FRAGMENT_NODE = 11;
  const unsigned short NOTATION_NODE = 12;
  [Constant] readonly attribute unsigned short nodeType;
  [Pure] readonly attribute DOMString nodeName;
  [Pure] readonly attribute DOMString baseURI;
  [Pure] readonly attribute boolean isConnected;
  [Pure] readonly attribute DOMString ownerDocument;
  [Pure] readonly attribute DOMString parentNode;
  [Pure] readonly attribute DOMString parentElement;
  [Pure] boolean hasChildNodes();
  [SameObject] readonly attribute DOMString childNodes;
  [Pure] readonly attribute DOMString firstChild;
  [Pure] readonly attribute DOMString lastChild;
  [Pure] readonly attribute DOMString previousSibling;
  [Pure] readonly attribute DOMString nextSibling;
  [CEReactions, Pure, SetterThrows] attribute DOMString nodeValue;
  [CEReactions, Pure, SetterThrows] attribute DOMString textContent;
  [CEReactions] undefined normalize();
  [CEReactions, Throws] DOMString cloneNode(boolean deep);
  [Pure] boolean isEqualNode(DOMString node);
  [Pure] boolean isSameNode(DOMString otherNode);
  const unsigned short DOCUMENT_POSITION_DISCONNECTED = 0x01;
  const unsigned short DOCUMENT_POSITION_PRECEDING = 0x02;
  const unsigned short DOCUMENT_POSITION_FOLLOWING = 0x04;
  const unsigned short DOCUMENT_POSITION_CONTAINS = 0x08;
  const unsigned short DOCUMENT_POSITION_CONTAINED_BY = 0x10;
  const unsigned short DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC = 0x20;
  [Pure] unsigned short compareDocumentPosition(DOMString other);
  [Pure] boolean contains(DOMString other);
  [Pure] DOMString lookupPrefix(DOMString namespace);
  [Pure] DOMString lookupNamespaceURI(DOMString prefix);
  [Pure] boolean isDefaultNamespace(DOMString namespace);
  [CEReactions, Throws] DOMString insertBefore(DOMString node, DOMString child);
  [CEReactions, Throws] DOMString appendChild(DOMString node);
  [CEReactions, Throws] DOMString replaceChild(DOMString node, DOMString child);
  [CEReactions, Throws] DOMString removeChild(DOMString child);
};
