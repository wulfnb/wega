// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface NamedNodeMap {
 readonly attribute unsigned long length;
 getter Attr? item(unsigned long index);
 getter Attr? getNamedItem(DOMString name);
 Attr? getNamedItemNS(DOMString? namespace, DOMString localName);
 Attr? setNamedItem(Attr attr);
 Attr? setNamedItemNS(Attr attr);
 Attr removeNamedItem(DOMString name);
 Attr removeNamedItemNS(DOMString? namespace, DOMString localName);
};
