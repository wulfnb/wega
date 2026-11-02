// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface HTMLCollection {
 readonly attribute unsigned long length;
 getter Element? item(unsigned long index);
 getter Element? namedItem(DOMString name);
 iterable<Element>;
};
