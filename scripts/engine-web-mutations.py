#!/usr/bin/env python3
"""Prove the web-correctness rules by breaking one rule, running its test, then restoring.

Covers CORS/SameSite/CSP/Referer, script loading, forms, encodings, history, input
and DOM budgets. Like engine-p0-mutations.py: never interrupts restoration, and
rebuilds the original sources at the end.
"""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
WEB = 'engines/wega/src/web.rs'
VIEW = 'engines/wega/src/view.rs'
DOM = 'engines/wega/src/dom.rs'
V8 = 'engines/wega/src/v8.rs'
BINDINGS = 'engines/wega/src/dom/bindings.js'
OBSERVERS = 'engines/wega/src/dom/observers.js'

# (name, [(file, before, after)], test target, test name)
MUTATIONS = [
    ('cors-requires-allow-origin',
     [(WEB, 'if (allowed === null) return "no Access-Control-Allow-Origin header";', 'if (allowed === null) return null;')],
     'security', 'cross_origin_reads_need_access_control_allow_origin'),
    ('cors-wildcard-never-with-credentials',
     [(WEB, 'if (allowed === "*") return "a wildcard origin is not allowed with credentials";', 'if (allowed === "*") return null;')],
     'security', 'cross_origin_reads_need_access_control_allow_origin'),
    ('credentials-mode-controls-cookies',
     [(WEB, 'const wegaInclude = wegaCredentials === "include" ||', 'const wegaInclude = true ||')],
     'security', 'credentials_cross_origin_only_with_an_exact_origin_and_allow_credentials'),
    ('preflight-non-simple-requests',
     [(WEB, 'if (wegaCross && wegaMode === "cors" && wegaNeedsPreflight(req)) {', 'if (false) {')],
     'security', 'non_simple_requests_are_preflighted_and_respect_the_answer'),
    ('opaque-no-cors-responses',
     [(WEB, 'if (wegaCross && wegaMode === "no-cors" && !redirectStatus(resp.status)) {', 'if (false) {')],
     'security', 'modes_opaque_responses_and_exposed_headers'),
    ('cors-exposed-headers-only',
     [(WEB, 'resp.headers = wegaExposedHeaders(resp.headers, wegaInclude);', 'resp.headers = resp.headers;')],
     'security', 'modes_opaque_responses_and_exposed_headers'),
    ('samesite-cross-site-blocks-lax',
     [('engines/wega/src/cookies.rs', '(_, SiteContext::CrossSite) => false,', '(_, SiteContext::CrossSite) => true,')],
     'security', 'samesite_cookies_stay_on_their_site'),
    ('csp-inline-scripts',
     [(VIEW, '            .allows_inline_script(Some(&element.nonce), &element.text)\n        {', '            .allows_inline_script(Some(&element.nonce), &element.text)\n            && false\n        {')],
     'security', 'content_security_policy_governs_scripts_eval_and_connections'),
    ('csp-eval',
     [(VIEW, 'host.allow_eval(self.csp().allows_eval());', 'host.allow_eval(true);')],
     'security', 'content_security_policy_governs_scripts_eval_and_connections'),
    ('csp-connect-src',
     [(V8, '    if !dom\n        .csp\n', '    if false && !dom\n        .csp\n')],
     'security', 'content_security_policy_governs_scripts_eval_and_connections'),
    ('referrer-strict-origin-cross-origin',
     [('engines/wega/src/network.rs', '        _ => (!downgrade).then_some(origin),', '        _ => Some(full.to_string()),')],
     'security', 'referer_follows_strict_origin_when_cross_origin'),
    ('script-inserted-external-scripts',
     [(BINDINGS, '    host("scriptInserted", script._id, String(script._forceAsync === true));', '    void script;')],
     'scripts', 'script_inserted_external_scripts_run_and_fire_load'),
    ('async-false-keeps-order',
     [(VIEW, 'if request.force_async && self.ordered.len() > before {', 'if self.ordered.len() > before {')],
     'scripts', 'script_inserted_scripts_keep_order_only_when_async_is_false'),
    ('inline-inserted-scripts-are-synchronous',
     [(BINDINGS, '        __wega_run_script(text, host("url"));', '        queueMicrotask(() => __wega_run_script(text, host("url")));')],
     'scripts', 'inline_inserted_scripts_run_synchronously_as_classic_scripts'),
    ('fragment-parsed-scripts-never-run',
     [(DOM, 'self.started_scripts.borrow_mut().insert(*id);', 'let _ = id;')],
     'scripts', 'inline_inserted_scripts_run_synchronously_as_classic_scripts'),
    ('parser-pauses-at-scripts',
     [('engines/wega/src/parser.rs', 'html5ever::TokenizerResult::Script(node) => return Step::Script(node),', 'html5ever::TokenizerResult::Script(_) => continue,')],
     'scripts', 'parser_scripts_see_the_partial_document_and_document_write'),
    ('document-write-at-insertion-point',
     [(VIEW, 'Some(parser) if !parser.is_finished() => parser.write(&requests.writes),', 'Some(parser) if false => parser.write(&requests.writes),')],
     'scripts', 'parser_scripts_see_the_partial_document_and_document_write'),
    ('defer-runs-in-order-after-parsing',
     [(VIEW, '(true, true, _, _) | (true, false, false, true) => self.deferred.push_back(pending),', '(true, true, _, _) | (true, false, false, true) => self.asynchronous.push(pending),')],
     'scripts', 'defer_and_blocking_scripts_run_in_document_order_before_domcontentloaded'),
    ('load-waits-for-subresources',
     [(VIEW, '            && self.in_flight.load(Ordering::Relaxed) == 0\n', '\n')],
     'scripts', 'load_waits_for_images_and_fires_after_domcontentloaded'),
    ('script-size-limit-is-resource-limit',
     [(VIEW, 'let state = match self.fetch(url.clone(), self.limits.resource_bytes, context) {', 'let state = match self.fetch(url.clone(), 1024 * 1024, context) {')],
     'scripts', 'large_scripts_run_and_slow_scripts_are_reported_not_killed'),
    ('slow-scripts-are-reported',
     [(V8, 'if !fired && elapsed > self.slow {', 'if false {')],
     'scripts', 'large_scripts_run_and_slow_scripts_are_reported_not_killed'),
    ('no-silent-script-cap',
     [(VIEW, 'const MAX_SCRIPTS: usize = 1024;', 'const MAX_SCRIPTS: usize = 64;')],
     'scripts', 'more_than_sixty_four_scripts_all_run'),
    ('post-forms-load-in-the-worker',
     [(VIEW, 'if options.method == blitz_traits::net::http::Method::GET {', 'if true {')],
     'forms', 'clicking_submit_posts_an_urlencoded_body_and_shows_the_response'),
    ('submit-event-is-cancelable',
     [(VIEW, '                if !cancelled {\n                    self.document.borrow().submit_form(form, submitter);', '                if true {\n                    self.document.borrow().submit_form(form, submitter);')],
     'forms', 'submit_events_can_cancel_and_submit_skips_them'),
    ('formdata-from-form',
     [('engines/wega/src/dom/forms.js', '        this.append(name, value);', '        void [name, value];')],
     'forms', 'multipart_and_formdata_use_the_form_entry_list'),
    ('implicit-submission',
     [(VIEW, 'if key == "Enter" && self.implicit_submission(target) {', 'if false {')],
     'forms', 'enter_in_a_single_field_submits_its_form'),
    ('documents-are-decoded-by-charset',
     [(VIEW, '            DocumentKind::Html => {\n                let (html, encoding) =\n                    encoding::decode_html(&response.bytes, response.header("content-type"));', '            DocumentKind::Html => {\n                let (html, encoding) = (\n                    String::from_utf8_lossy(&response.bytes).into_owned(),\n                    encoding_rs::UTF_8,\n                );')],
     'documents', 'transport_meta_and_bom_charsets_decode_documents'),
    ('files-are-downloads',
     [('engines/wega/src/encoding.rs', '        Some(_) => DocumentKind::Download,', '        Some(_) => DocumentKind::Html,')],
     'documents', 'text_images_and_error_statuses_are_shown_and_files_are_downloads'),
    ('refresh-navigates',
     [(VIEW, '            self.refresh = None;\n            self.navigate.borrow_mut().push(url);', '            self.refresh = None;\n            let _ = url;')],
     'documents', 'refresh_headers_navigate_after_load'),
    ('back-walks-document-history',
     [(OBSERVERS, '__wega_traverse: { value: (delta) => traverse(Number(delta)) },', '__wega_traverse: { value: (delta) => false },')],
     'history', 'browser_back_walks_pushstate_entries_before_leaving_the_document'),
    ('fragments-never-refetch',
     [(VIEW, '        if self.same_document(&url) {\n            self.navigate_fragment(&url);\n            return Ok(());', '        if false {\n            self.navigate_fragment(&url);\n            return Ok(());')],
     'history', 'fragments_scroll_fire_hashchange_and_never_refetch'),
    ('hover-follows-the-pointer',
     [(VIEW, '            self.document.borrow_mut().set_hover_to(page_x, page_y);', '            let _ = (page_x, page_y);')],
     'input', 'hover_applies_css_and_events_and_reports_the_cursor'),
    ('events-reach-ancestors',
     [(WEB, '"    if (data === undefined || (!data.listeners[event.type] && !isNode(self))) {"', '"    if (data === undefined || !data.listeners[event.type]) {"')],
     'input', 'events_reach_ancestors_when_the_target_has_no_listeners'),
    ('nodes-have-listener-data',
     [(BINDINGS, '      web?.event.setEventTargetData(this);', '      void web;')],
     'input', 'events_reach_ancestors_when_the_target_has_no_listeners'),
    ('wheel-uses-dom-direction',
     [(VIEW, 'doc.scroll_node_by(target, -pointer.delta_x, -pointer.delta_y, |_| {});', 'doc.scroll_node_by(target, pointer.delta_x, pointer.delta_y, |_| {});')],
     'input', 'wheel_scrolls_the_container_under_the_pointer_unless_cancelled'),
    ('dialogs-reach-the-browser',
     [(BINDINGS, '      host("dialog", "confirm", String(message), "") === "true",', '      false,')],
     'input', 'dialogs_block_for_the_browser_answer'),
    ('popups-need-user-activation',
     [(DOM, 'if !active || !matches!(url.scheme(), "http" | "https") || requests.opened.len() >= 4 {', 'if !matches!(url.scheme(), "http" | "https") || requests.opened.len() >= 4 {')],
     'input', 'popups_need_a_user_gesture_and_target_blank_links_open_windows'),
    ('target-blank-opens-a-window',
     [(VIEW, '                            action = Some(Action::NewWindow(url));', '                            action = None;')],
     'input', 'popups_need_a_user_gesture_and_target_blank_links_open_windows'),
    ('detached-text-is-credited',
     [(DOM, '        self.used.set(self.used.get().saturating_sub(bytes));', '        let _ = bytes;')],
     'pages', 'replacing_content_returns_its_node_and_text_budget'),
    ('replaced-text-is-credited',
     [(DOM, '                        credit(data.content.len());', '                        let _ = &data;')],
     'pages', 'replacing_content_returns_its_node_and_text_budget'),
    ('parser-node-budget',
     [('engines/wega/src/parser.rs', '        if self.created.get() <= self.limit {', '        if true {')],
     'pages', 'native_dom_budget_and_cycles_are_rejected'),
    ('shared-watchdog-terminates',
     [(V8, '                                handle.terminate_execution();', '                                let _ = &handle;')],
     'errors', 'deadline_remains_fatal_and_a_new_document_recovers'),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--target-dir', default=str(ROOT / 'target/web-mutations'))
    parser.add_argument('--output', default=str(ROOT / 'target/web-mutations.json'))
    parser.add_argument('--only', help='run one mutation by name')
    args = parser.parse_args()
    results = []
    for name, patches, target, test in MUTATIONS:
        if args.only and name != args.only:
            continue
        originals = {ROOT / relative: (ROOT / relative).read_text() for relative, _, _ in patches}
        try:
            for relative, before, after in patches:
                path = ROOT / relative
                source = path.read_text()
                if source.count(before) != 1:
                    raise RuntimeError(f'{name}: mutation anchor must match exactly once in {relative}')
                path.write_text(source.replace(before, after, 1))
            run = subprocess.run(
                ['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'engines/Cargo.toml'),
                 '--target-dir', args.target_dir, '--jobs', '4', '--test', target, test, '--', '--exact'],
                cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            detected = run.returncode != 0 and f'test {test} ... FAILED' in run.stdout and '1 failed' in run.stdout
            results.append({'mutation': name, 'test': f'{target}::{test}', 'detected': detected})
            print(f'{name}: {"DETECTED" if detected else "FAILED TO PROVE"}', flush=True)
            if not detected:
                print(run.stdout[-4000:])
                raise RuntimeError(name)
        finally:
            for path, source in originals.items():
                path.write_text(source)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(results, indent=2) + '\n')
    # Rebuild original sources after the final mutation; mutant artifacts are never used for a gate.
    subprocess.run(['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'engines/Cargo.toml'),
                    '--target-dir', args.target_dir, '--jobs', '4', '--no-run'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
