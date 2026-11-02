// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface DOMTokenList {
 readonly attribute unsigned long length;
 getter DOMString? item(unsigned long index);
 boolean contains(DOMString token);
 undefined add(DOMString... tokens);
 undefined remove(DOMString... tokens);
 boolean toggle(DOMString token, optional boolean force);
 boolean replace(DOMString token, DOMString newToken);
 boolean supports(DOMString token);
 attribute DOMString value;
 stringifier;
 iterable<DOMString>;
};
