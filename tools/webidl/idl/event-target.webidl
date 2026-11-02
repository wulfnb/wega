// Wega contributors, 2026. Audited DOM/HTML binding surface; algorithms in semantic adapters.
callback interface EventListener { undefined handleEvent(Event event); };
dictionary EventListenerOptions { boolean capture = false; };
dictionary AddEventListenerOptions : EventListenerOptions { boolean once = false; boolean passive; AbortSignal signal; };
[Exposed=Window] interface EventTarget {
 constructor();
 undefined addEventListener(DOMString type, EventListener? callback, optional (AddEventListenerOptions or boolean) options = {});
 undefined removeEventListener(DOMString type, EventListener? callback, optional (EventListenerOptions or boolean) options = {});
 boolean dispatchEvent(Event event);
};
