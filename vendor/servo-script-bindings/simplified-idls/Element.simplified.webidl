/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
/*
 * Simplified Element.webidl for the Wega WebIDL generator pipeline.
 * Stripped constructs (deferred to JS layer):
 *   - Nullable interface types (namespaceURI?, prefix?, etc.) → DOMString (node ID)
 *   - Dictionary parameters (ShadowRootInit, ScrollToOptions, etc.) → DOMString
 *   - Union types ((TrustedType or DOMString), etc.) → DOMString
 *   - sequence<DOMString> returns → DOMString
 *   - Interface-typed params/returns (Attr, Element?, Node, etc.) → DOMString
 *   - [PutForwards=value], [Unscopable], Promise<undefined>
 *
 * Full-sig constructs that JS layer must handle:
 *   - DOMTokenList wrapping for classList/part (from [PutForwards=value])
 *   - NamedNodeMap wrapping for attributes ([SameObject])
 *   - ShadowRoot wrapping for attachShadow (dictionary init, [NewObject])
 *   - DOMRect wrapping for getBoundingClientRect ([NewObject])
 *   - TrustedType/TrustedHTML union handling in setAttribute/innerHTML
 *   - LegacyNullToEmptyString in innerHTML/outerHTML
 *   - Full DOMTokenList iteration, querySelector/querySelectorAll return NodeList
 *   - HTMLCollection returns for getElementsByTagName/NS/ClassName
 *
 * Mixin includes (members provided by native base, not generated):
 *   Element includes ChildNode;
 *   Element includes NonDocumentTypeChildNode;
 *   Element includes ParentNode;
 *   Element includes ActivatableElement;
 *   Element includes ARIAMixin;
 *   Element includes Animatable;
 */

[Exposed=Window]
interface Element : Node {
  constructor(DOMString localName);

  [Constant]
  readonly attribute DOMString namespaceURI;
  [Constant]
  readonly attribute DOMString prefix;
  [Constant]
  readonly attribute DOMString localName;
  [Pure]
  readonly attribute DOMString tagName;

  [CEReactions, Pure]
           attribute DOMString id;
  [CEReactions, Pure]
           attribute DOMString className;
  [SameObject]
  readonly attribute DOMString classList;
  [CEReactions] attribute DOMString slot;

  [Pure]
  boolean hasAttributes();
  [SameObject]
  readonly attribute DOMString attributes;
  [Pure]
  DOMString getAttributeNames();
  [Pure]
  DOMString getAttribute(DOMString name);
  [Pure]
  DOMString getAttributeNS(DOMString namespace, DOMString localName);
  [CEReactions, Throws]
  boolean toggleAttribute(DOMString name, optional boolean force = false);
  [CEReactions, Throws]
  undefined setAttribute(DOMString name, DOMString value);
  [CEReactions, Throws]
  undefined setAttributeNS(DOMString namespace, DOMString name, DOMString value);
  [CEReactions]
  undefined removeAttribute(DOMString name);
  [CEReactions]
  undefined removeAttributeNS(DOMString namespace, DOMString name);
  boolean hasAttribute(DOMString name);
  boolean hasAttributeNS(DOMString namespace, DOMString name);

  [Pure, Throws]
  DOMString closest(DOMString selectors);
  [Pure, Throws]
  boolean matches(DOMString selectors);
  [Pure, Throws]
  boolean webkitMatchesSelector(DOMString selectors);

  DOMString getElementsByTagName(DOMString localName);
  DOMString getElementsByTagNameNS(DOMString namespace, DOMString localName);
  DOMString getElementsByClassName(DOMString classNames);

  [CEReactions, Throws]
  DOMString insertAdjacentElement(DOMString where_, DOMString element);
  [Throws]
  undefined insertAdjacentText(DOMString where_, DOMString data);
  [CEReactions, Throws]
  undefined insertAdjacentHTML(DOMString position, DOMString string);

  [Throws] undefined attachShadow(DOMString init);
  readonly attribute DOMString shadowRoot;

  readonly attribute DOMString customElementRegistry;

  DOMString getClientRects();
  [NewObject]
  DOMString getBoundingClientRect();

  undefined scrollIntoView(optional DOMString arg = "");
  undefined scroll(optional DOMString options = "");
  undefined scrollTo(optional DOMString options = "");
  undefined scrollBy(optional DOMString options = "");
  attribute double scrollTop;
  attribute double scrollLeft;
  readonly attribute unsigned long scrollWidth;
  readonly attribute unsigned long scrollHeight;
  readonly attribute unsigned long clientTop;
  readonly attribute unsigned long clientLeft;
  readonly attribute unsigned long clientWidth;
  readonly attribute unsigned long clientHeight;
  readonly attribute double currentCSSZoom;

  [CEReactions, Throws]
  undefined setHTMLUnsafe(DOMString html);
  DOMString getHTML(optional DOMString options = "");
  [CEReactions, Throws]
  attribute DOMString innerHTML;
  [CEReactions, Throws]
  attribute DOMString outerHTML;
};
