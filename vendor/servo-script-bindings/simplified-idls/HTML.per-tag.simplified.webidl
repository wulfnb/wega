/* Wega contributors, 2026. Simplified HTML per-tag element interfaces for the Wega WebIDL generator pipeline.
 * Generated automatically from vendor/servo-script-bindings/webidls/HTML*.webidl.
 *
 * Simplification rules (complex constructs deferred to JS layer):
 *   - [HTMLConstructor] → [Throws] constructor(DOMString _unused)
 *   - USVString / ByteString → DOMString
 *   - Union types (TrustedHTML or DOMString) → DOMString
 *   - Promise<...> → undefined
 *   - Callback params → DOMString stub
 *   - Pref-conditional members → removed
 *   - LegacyNullToEmptyString on setters → handled in JS
 *   - static methods (supports) → removed
 *   - LegacyFactoryFunction → removed
 *   - callback / enum / typedef / stringifier → removed
 *   - obsolete members (per-element obsolete list) → removed
 *   - interface-typed params/returns → kept (generator handles them, runtime wraps)
 *
 * Mixin includes (WindowEventHandlers, HTMLHyperlinkElementUtils, LinkStyle)
 *   → handled in JS layer via event wiring / semantic adapters
 */

// Base class: HTMLElement already exists via semantic adapter
// All per-tag elements extend HTMLElement


[Exposed=Window]
interface HTMLAnchorElement : HTMLElement {
};

[Exposed=Window]
interface HTMLAreaElement : HTMLElement {
};

[Exposed=Window]
interface HTMLAudioElement : HTMLElement {
};

[Exposed=Window]
interface HTMLBRElement : HTMLElement {
};

[Exposed=Window]
interface HTMLBaseElement : HTMLElement {
};

[Exposed=Window]
interface HTMLBodyElement : HTMLElement {
};

[Exposed=Window]
interface HTMLButtonElement : HTMLElement {
};

[Exposed=Window]
interface HTMLCanvasElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDListElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDataElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDataListElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDetailsElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDialogElement : HTMLElement {
};

[Exposed=Window]
interface HTMLDivElement : HTMLElement {
};

[Exposed=Window]
interface HTMLEmbedElement : HTMLElement {
};

[Exposed=Window]
interface HTMLFieldSetElement : HTMLElement {
};

[Exposed=Window]
interface HTMLFontElement : HTMLElement {
};

[Exposed=Window]
interface HTMLFormElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHRElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHeadElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHeadingElement : HTMLElement {
};

[Exposed=Window]
interface HTMLHtmlElement : HTMLElement {
};

[Exposed=Window]
interface HTMLIFrameElement : HTMLElement {
};

[Exposed=Window]
interface HTMLImageElement : HTMLElement {
};

[Exposed=Window]
interface HTMLInputElement : HTMLElement {
};

[Exposed=Window]
interface HTMLLIElement : HTMLElement {
};

[Exposed=Window]
interface HTMLLabelElement : HTMLElement {
};

[Exposed=Window]
interface HTMLLegendElement : HTMLElement {
};

[Exposed=Window]
interface HTMLLinkElement : HTMLElement {
};

[Exposed=Window]
interface HTMLMapElement : HTMLElement {
};

[Exposed=Window]
interface HTMLMarqueeElement : HTMLElement {
};

[Exposed=Window]
interface HTMLMetaElement : HTMLElement {
};

[Exposed=Window]
interface HTMLMeterElement : HTMLElement {
};

[Exposed=Window]
interface HTMLModElement : HTMLElement {
};

[Exposed=Window]
interface HTMLOListElement : HTMLElement {
};

[Exposed=Window]
interface HTMLObjectElement : HTMLElement {
};

[Exposed=Window]
interface HTMLOptGroupElement : HTMLElement {
};

[Exposed=Window]
interface HTMLOutputElement : HTMLElement {
};

[Exposed=Window]
interface HTMLParagraphElement : HTMLElement {
};

[Exposed=Window]
interface HTMLParamElement : HTMLElement {
};

[Exposed=Window]
interface HTMLPictureElement : HTMLElement {
};

[Exposed=Window]
interface HTMLPreElement : HTMLElement {
};

[Exposed=Window]
interface HTMLProgressElement : HTMLElement {
};

[Exposed=Window]
interface HTMLQuoteElement : HTMLElement {
};

[Exposed=Window]
interface HTMLScriptElement : HTMLElement {
};

[Exposed=Window]
interface HTMLSelectElement : HTMLElement {
};

[Exposed=Window]
interface HTMLSlotElement : HTMLElement {
};

[Exposed=Window]
interface HTMLSourceElement : HTMLElement {
};

[Exposed=Window]
interface HTMLSpanElement : HTMLElement {
};

[Exposed=Window]
interface HTMLStyleElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableCaptionElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableCellElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableColElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableRowElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTableSectionElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTemplateElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTextAreaElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTimeElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTitleElement : HTMLElement {
};

[Exposed=Window]
interface HTMLTrackElement : HTMLElement {
};

[Exposed=Window]
interface HTMLUListElement : HTMLElement {
};

[Exposed=Window]
interface HTMLUnknownElement : HTMLElement {
};

[Exposed=Window]
interface HTMLVideoElement : HTMLElement {
};
