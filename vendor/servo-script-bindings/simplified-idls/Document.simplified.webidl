/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
/*
 * Simplified Document.webidl for the Wega WebIDL generator pipeline.
 * Stripped constructs (deferred to JS layer):
 *   - Dictionary parameters (ElementCreationOptions, ImportNodeOptions, etc.)
 *   - Union types ((TrustedHTML or DOMString), etc.) → DOMString
 *   - Interface-typed params/returns (DocumentType?, Element?, Node, etc.) → DOMString
 *   - Sequence returns (sequence<Element>) → DOMString
 *   - [PutForwards=href], [LegacyUnforgeable], [LegacyLenientSetter/This]
 *   - Promise<undefined> (exitFullscreen) → undefined
 *   - [Pref="..."] prefixed features → stub without attribute
 *   - EventHandler attributes (→ string callbacks)
 *   - static methods (parseHTMLUnsafe, parseHTML) → non-static
 *   - "any" type (adoptedStyleSheets) → DOMString
 *   - "getter" members (NamedPropertyValue) → stub as regular method
 *   - Servo-internal API (servoGetMediaControls)
 *
 * Mixin includes (members from native_base, NOT generated):
 *   Document includes NonElementParentNode;
 *   Document includes ParentNode;
 *   Document includes DocumentOrShadowRoot;
 *   Document includes GlobalEventHandlers;
 *
 * Full-sig constructs JS layer must handle:
 *   - DOMImplementation wrapping for implementation
 *   - NamedNodeMap wrapping for all collections (images, embeds, plugins, etc.)
 *   - Location wrapping for location (PutForwards=href)
 *   - CSSStyleSheet wrapping for styleSheets (from DocumentOrShadowRoot mixin)
 *   - TrustedHTML/TrustedType union in open/write/writeln/execCommand
 *   - LegacyNullToEmptyString on bgColor/fgColor
 *   - LegacyLenientSetter on fullscreen/readyState
 *   - LegacyLenientThis on onreadystatechange
 *   - LegacyUnforgeable on domain/referrer/cookie
 *   - DocumentTimeline wrapping for timeline
 *   - EventHandler IDL → actual event handler functions
 *   - Static method → non-static (parseHTML)
 *   - Getter (NamedPropertyValue) → document[name] indexed access
 *   - USVString (URL, documentURI) → DOMString
 */

[Exposed=Window]
interface Document : Node {
  [Throws] constructor(DOMString unused);

  [SameObject]
  readonly attribute DOMString implementation;
  [Constant]
  readonly attribute DOMString URL;
  [Constant]
  readonly attribute DOMString documentURI;
  readonly attribute DOMString compatMode;
  readonly attribute DOMString characterSet;
  readonly attribute DOMString charset;
  readonly attribute DOMString inputEncoding;
  [Constant]
  readonly attribute DOMString contentType;

  readonly attribute DOMString doctype;
  readonly attribute DOMString documentElement;

  DOMString getElementsByTagName(DOMString qualifiedName);
  DOMString getElementsByTagNameNS(DOMString namespace, DOMString qualifiedName);
  DOMString getElementsByClassName(DOMString classNames);

  DOMString createElement(DOMString localName, optional DOMString options = "");
  DOMString createElementNS(DOMString namespace, DOMString qualifiedName, optional DOMString options = "");
  undefined createDocumentFragment();
  undefined createTextNode(DOMString data);
  undefined createCDATASection(DOMString data);
  undefined createComment(DOMString data);
  undefined createProcessingInstruction(DOMString target, DOMString data);

  undefined importNode(DOMString node, optional boolean options = false);
  undefined adoptNode(DOMString node);

  undefined createAttribute(DOMString localName);
  undefined createAttributeNS(DOMString namespace, DOMString qualifiedName);

  undefined createEvent(DOMString interface_);

  undefined createRange();

  undefined createNodeIterator(DOMString root, optional unsigned long whatToShow = 4294967295, optional DOMString filter = "");
  undefined createTreeWalker(DOMString root, optional unsigned long whatToShow = 4294967295, optional DOMString filter = "");

  // Resource metadata (partial sealed interface Document)
  [SetterThrows] attribute DOMString domain;
  readonly attribute DOMString referrer;
  [Throws] attribute DOMString cookie;
  readonly attribute DOMString lastModified;
  readonly attribute DOMString readyState;
  readonly attribute DOMString location;

  // DOM tree accessors
  DOMString namedItem(DOMString name);
  [CEReactions] attribute DOMString title;
  [CEReactions] attribute DOMString dir;
  [CEReactions, SetterThrows] attribute DOMString body;
  readonly attribute DOMString head;
  [SameObject] readonly attribute DOMString images;
  [SameObject] readonly attribute DOMString embeds;
  [SameObject] readonly attribute DOMString plugins;
  [SameObject] readonly attribute DOMString links;
  [SameObject] readonly attribute DOMString forms;
  [SameObject] readonly attribute DOMString scripts;
  DOMString getElementsByName(DOMString elementName);
  readonly attribute DOMString currentScript;

  // Dynamic markup insertion
  [CEReactions, Throws] DOMString open();
  [CEReactions, Throws] undefined close();
  [CEReactions, Throws] undefined write(DOMString... text);
  [CEReactions, Throws] undefined writeln(DOMString... text);

  // User interaction
  readonly attribute DOMString defaultView;
  boolean hasFocus();
  boolean execCommand(DOMString commandId, optional boolean showUI = false, optional DOMString value = "");
  boolean queryCommandEnabled(DOMString commandId);
  boolean queryCommandIndeterm(DOMString commandId);
  boolean queryCommandState(DOMString commandId);
  boolean queryCommandSupported(DOMString commandId);
  DOMString queryCommandValue(DOMString commandId);
  readonly attribute boolean hidden;
  readonly attribute DOMString visibilityState;

  // Readystatechange (partial sealed interface Document)
  attribute DOMString onreadystatechange;

  // Obsolete colors (partial interface Document)
  [CEReactions] attribute DOMString fgColor;
  [CEReactions] attribute DOMString bgColor;
  [SameObject] readonly attribute DOMString anchors;
  [SameObject] readonly attribute DOMString applets;
  undefined clear();
  undefined captureEvents();
  undefined releaseEvents();

  // Fullscreen (partial interface Document)
  readonly attribute boolean fullscreenEnabled;
  readonly attribute boolean fullscreen;
  undefined exitFullscreen();
  attribute DOMString onfullscreenchange;
  attribute DOMString onfullscreenerror;

  // CSSOM-View (partial interface Document)
  readonly attribute DOMString scrollingElement;

  // Selection API (partial interface Document)
  DOMString getSelection();

  // Web Animations (partial interface Document)
  readonly attribute DOMString timeline;

  // Sanitizer API (partial interface Document)
  [Throws] DOMString parseHTML(DOMString html, optional DOMString options = "");
};
