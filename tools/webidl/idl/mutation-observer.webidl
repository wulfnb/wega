// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
callback MutationCallback = undefined (sequence<MutationRecord> mutations, MutationObserver observer);
dictionary MutationObserverInit {
 boolean childList = false;
 boolean attributes;
 boolean characterData;
 boolean subtree = false;
 boolean attributeOldValue;
 boolean characterDataOldValue;
 sequence<DOMString> attributeFilter;
};
[Exposed=Window] interface MutationObserver {
 constructor(MutationCallback callback);
 undefined observe(Node target, optional MutationObserverInit options = {});
 undefined disconnect();
 sequence<MutationRecord> takeRecords();
};
