// Servo 0.7 reports failed loads as one unstyled line ("Could not load the
// requested page: <reason>") that is nearly invisible with a dark color scheme
// and offers no way forward. Replace exactly that document with a readable
// page, a plain-language explanation and Try again / Go back actions.
(() => {
  const PREFIX = 'Could not load the requested page: ';
  const CSS = `
    :root { color-scheme: light dark; --bg: #ffffff; --text: #1d1f23; --muted: #5a606b; --accent: #0b6b55; --on-accent: #ffffff; --line: #d6d9de; }
    @media (prefers-color-scheme: dark) { :root { --bg: #16181d; --text: #e9ebef; --muted: #a9afba; --accent: #93f5d5; --on-accent: #061b18; --line: #343842; } }
    html { background: var(--bg); }
    body { margin: 0; font: 15px/1.55 system-ui, sans-serif; color: var(--text); background: var(--bg); }
    main { max-width: 34rem; margin: 18vh auto 0; padding: 0 24px; }
    h1 { font-size: 1.6rem; margin: 0 0 .5rem; }
    p { color: var(--muted); margin: 0 0 1.25rem; }
    .actions { display: flex; gap: .6rem; margin-bottom: 1.5rem; }
    button { font: inherit; padding: .5rem 1rem; border-radius: 8px; border: 1px solid var(--line); background: transparent; color: var(--text); cursor: pointer; }
    button.primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
    details { color: var(--muted); font-size: .85rem; }
    code { display: block; margin-top: .4rem; overflow-wrap: anywhere; }`;

  const explain = (reason, host) => {
    if (/certificate|tls|ssl/i.test(reason)) return `The secure connection to ${host} failed.`;
    if (/dns|resolve|lookup|not known/i.test(reason)) return `Wega couldn't find ${host}. Check the address for typos.`;
    return `${host} didn't respond. The site may be down, or you may be offline.`;
  };

  const upgrade = () => {
    const body = document.body;
    if (document.title !== 'Error loading page' || !body || body.children.length !== 1) return;
    const line = body.children[0];
    if (line.tagName !== 'P' || !line.textContent.startsWith(PREFIX)) return;
    const reason = line.textContent.slice(PREFIX.length);
    const host = location.hostname || location.href;
    const el = (tag, props, ...children) => {
      const element = document.createElement(tag);
      Object.assign(element, props);
      element.append(...children);
      return element;
    };
    const retry = el('button', {type: 'button', className: 'primary', textContent: 'Try again', onclick: () => location.reload()});
    const actions = el('div', {className: 'actions'}, retry);
    if (history.length > 1) actions.append(el('button', {type: 'button', textContent: 'Go back', onclick: () => history.back()}));
    document.head.append(el('style', {textContent: CSS}));
    document.title = `Can't reach ${host}`;
    body.textContent = '';
    body.append(el('main', {},
      el('h1', {textContent: "Can't reach this page"}),
      el('p', {textContent: explain(reason, host)}),
      actions,
      el('details', {}, el('summary', {textContent: 'Technical details'}), el('code', {textContent: reason}))));
    retry.focus();
  };

  // User scripts run when <head> is inserted. Servo builds its error document
  // after that point (and already reports readyState "complete"), so decide
  // once the body has its first element, then stop observing.
  if (document.body?.firstElementChild) {
    upgrade();
  } else {
    const observer = new MutationObserver(() => {
      if (!document.body?.firstElementChild) return;
      observer.disconnect();
      upgrade();
    });
    observer.observe(document, {childList: true, subtree: true});
  }
})();
