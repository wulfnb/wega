// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
[Exposed=Window] interface Location {
 attribute DOMString href;
 readonly attribute DOMString origin;
 attribute DOMString protocol;
 attribute DOMString host;
 attribute DOMString hostname;
 attribute DOMString port;
 attribute DOMString pathname;
 attribute DOMString search;
 attribute DOMString hash;
 undefined assign(DOMString url);
 undefined replace(DOMString url);
 undefined reload();
 stringifier;
};
