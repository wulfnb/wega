/* Simplified pin for step 08.b: Event family generator pipeline.
 * Source: vendor/servo-script-bindings/webidls/EventTarget.webidl
 * Divergence: simplified [Exposed] and method signatures to match
 * the generator's supported subset. Full signatures (callbacks,
 * unions, dictionaries) stay in the JS layer. */
[Exposed=Window]
interface EventTarget {
  [Throws] constructor(optional DOMString type = "");
  [Throws] undefined addEventListener(DOMString type);
  [Throws] undefined removeEventListener(DOMString type);
  undefined dispatchEvent(DOMString type);
};
