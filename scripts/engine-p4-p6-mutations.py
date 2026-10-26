#!/usr/bin/env python3
"""Prove P4-P6 regressions with targeted faults, restoring source after each run."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target-dir', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--desktop-target', help='Also prove edited fallback through the native UI')
    args = parser.parse_args()
    target = str(Path(args.target_dir).resolve())
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    mutations = [
        ('frozen-state-preservation', 'engines/wega/src/view.rs',
         'if freeze {\n            self.document.borrow_mut().release_image_cache();',
         'if freeze {\n            self.script = None;\n            self.document.borrow_mut().release_image_cache();',
         ['-p', 'wega-engine', '--test', 'pages'], 'startup_snapshot_is_private_per_document_and_freezing_preserves_live_state'),
        ('style-pool-cap', 'engines/vendor/blitz/blitz-dom/src/document.rs',
         'style_config::set_pref!("layout.threads", 2);', 'style_config::set_pref!("layout.threads", 4);',
         ['-p', 'wega-engine', '--test', 'pages'], 'parallel_style_threshold_preserves_updates_on_large_documents'),
        ('frame-credit-sequence', 'engines/wega/src/main.rs',
         '&& *credit == Some(sequence)', '&& true',
         ['-p', 'wega-engine', '--test', 'worker'], 'shared_frames_wait_for_exact_credits_and_resize_cannot_unfreeze_a_tab'),
        ('resize-keeps-freeze', 'engines/wega/src/main.rs',
         'view.request_redraw();', 'frozen = false; view.freeze(false);',
         ['-p', 'wega-engine', '--test', 'worker'], 'shared_frames_wait_for_exact_credits_and_resize_cannot_unfreeze_a_tab'),
        ('skia-color-alpha', 'engines/vendor/blitz/anyrender_skia/src/scene.rs',
         'self.cache.paint.set_alpha_f(self.cache.paint.alpha_f() * alpha);', 'self.cache.paint.set_alpha_f(alpha);',
         ['-p', 'wega-engine', '--test', 'css'], 'text_shadow_matches_positioned_glyph_reference'),
        ('container-size', 'engines/vendor/blitz/blitz-dom/src/stylo.rs',
         'Some(app_units::Au::from_f32_px(layout.content_box_width()))', 'None',
         ['-p', 'wega-engine', '--test', 'css'], 'inline_size_container_query_uses_content_size_and_updates_after_resize'),
        ('scheme-default-color', 'engines/vendor/blitz/blitz-dom/assets/default.css',
         ':root {\n    color: CanvasText;\n}', ':root {\n    color: black;\n}',
         ['-p', 'wega-engine', '--test', 'css'], 'default_text_color_follows_the_root_color_scheme_and_author_colors_win'),
        ('broker-resource-concurrency', 'engines/wega/src/network.rs',
         'if brokered { 2 } else { 4 }', 'if brokered { 1 } else { 4 }',
         ['-p', 'wega-engine', '--test', 'scripts'], 'script_inserted_scripts_keep_order_only_when_async_is_false'),
        ('offline-cookie-state', 'engines/network/src/lib.rs',
         'if script {\n                    policy.offline = false;\n                }\n                let value = if policy.permits(&policy_url)',
         'if false { policy.offline = false; }\n                let value = if policy.permits(&policy_url)',
         ['-p', 'wega-network', '--lib'], 'tests::capabilities_partition_profiles_share_live_cookies_and_track_policy'),
        ('cache-only-never-requests-network', "engines/network/src/lib.rs", 'if request.cache == "only-if-cached" {', 'if false {',
         ["-p", "wega-network", "--test", "broker"], 'real_http_is_shared_but_credentials_policy_and_cache_are_profile_partitioned'),
        ('reload-updates-public-cache', "engines/network/src/lib.rs", 'if cacheable && let Some((cache, _)) = cache {', 'if cacheable && request.cache != "reload" && let Some((cache, _)) = cache {',
         ["-p", "wega-network", "--test", "broker"], 'real_http_is_shared_but_credentials_policy_and_cache_are_profile_partitioned'),
    ]
    results = []
    for name, relative, before, after, flags, test in mutations:
        path = ROOT / relative
        original = path.read_text()
        try:
            if original.count(before) != 1:
                raise RuntimeError(f'{name}: expected one mutation anchor')
            path.write_text(original.replace(before, after, 1))
            run = subprocess.run(['cargo', 'test', '--offline', '--locked', '--manifest-path',
                                  str(ROOT / 'engines/Cargo.toml'), '--target-dir', target,
                                  '--jobs', '2', *flags, test, '--', '--exact'], cwd=ROOT,
                                 env={**os.environ, **({'WEGA_TEST_BROKER': '1'} if name == 'broker-resource-concurrency' else {})},
                                 text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output.parent / f'{name}.log').write_text(run.stdout)
            detected = run.returncode != 0 and f'test {test} ... FAILED' in run.stdout
            results.append({'mutation': name, 'test': test, 'detected': detected})
            print(f'{name}: {"DETECTED" if detected else "FAILED TO PROVE"}', flush=True)
            if not detected:
                raise RuntimeError(name)
        finally:
            path.write_text(original)
    if args.desktop_target:
        path = ROOT / 'src/wega_ui.rs'
        original = path.read_text()
        environment = {**os.environ, 'CARGO_TARGET_DIR': str(Path(args.desktop_target).resolve()),
                       'CARGO_INCREMENTAL': '0'}
        build = ['cargo', 'build', '--offline', '--locked', '--features', 'test-harness',
                 '--bin', 'wega', '--jobs', '2']
        try:
            before = 'if interacted {'
            if original.count(before) != 1:
                raise RuntimeError('edited-fallback: expected one mutation anchor')
            path.write_text(original.replace(before, 'if false {', 1))
            subprocess.run(build, cwd=ROOT, env=environment, check=True, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)
            environment['WEGA_ENGINE_BINARY'] = str(Path(target) / 'debug/wega-engine')
            # Rebuild the unmutated worker after the preceding worker faults.
            subprocess.run(['cargo', 'build', '--offline', '--locked', '--manifest-path',
                            str(ROOT / 'engines/Cargo.toml'), '--target-dir', target,
                            '-p', 'wega-engine', '--jobs', '2'], cwd=ROOT, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            run = subprocess.run(['python3', 'scripts/native-tests.py', '--suite', 'engine',
                                  '--binary', str(Path(args.desktop_target).resolve() / 'debug/wega'),
                                  '--output', str(output.parent / 'edited-fallback-native')],
                                 cwd=ROOT, env=environment, text=True, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT)
            (output.parent / 'edited-fallback.log').write_text(run.stdout)
            detected = run.returncode != 0 and 'Engine UI test timed out at stage 15' in run.stdout
            results.append({'mutation': 'edited-fallback', 'test': 'native engine stage 15', 'detected': detected})
            print(f'edited-fallback: {"DETECTED" if detected else "FAILED TO PROVE"}', flush=True)
            if not detected:
                raise RuntimeError('edited-fallback')
        finally:
            path.write_text(original)
            subprocess.run(build, cwd=ROOT, env=environment, check=True, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)
    output.write_text(json.dumps(results, indent=2) + '\n')
    subprocess.run(['cargo', 'test', '--offline', '--locked', '--workspace', '--manifest-path',
                    str(ROOT / 'engines/Cargo.toml'), '--target-dir', target, '--jobs', '2'],
                   cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
