// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface NodeList {
 readonly attribute unsigned long length;
 getter Node? item(unsigned long index);
 iterable<Node>;
};
