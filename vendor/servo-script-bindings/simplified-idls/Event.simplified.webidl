/* Simplified pin for step 08.b: Event family generator pipeline.
 * Source: vendor/servo-script-bindings/webidls/Event.webidl
 * Divergence: simplified [Exposed], constructor params, and method
 * signatures to match the generator's supported subset. Full
 * signatures (EventInit dictionary, nullable interface returns,
 * sequence<EventTarget>) stay in the JS layer. */
[Exposed=Window]
interface Event {
  [Throws] constructor(DOMString type);
  [Pure] readonly attribute DOMString type;
  const unsigned short NONE = 0;
  const unsigned short CAPTURING_PHASE = 1;
  const unsigned short AT_TARGET = 2;
  const unsigned short BUBBLING_PHASE = 3;
  readonly attribute unsigned short eventPhase;
  undefined stopPropagation();
  attribute boolean cancelBubble;
  undefined stopImmediatePropagation();
  [Pure] readonly attribute boolean bubbles;
  [Pure] readonly attribute boolean cancelable;
  attribute boolean returnValue;
  undefined preventDefault();
  [Pure] readonly attribute boolean defaultPrevented;
  [Pure] readonly attribute boolean composed;
  [Pure] readonly attribute boolean isTrusted;
  [Constant] readonly attribute double timeStamp;
  undefined initEvent(DOMString type);
};
