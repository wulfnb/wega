"""Worker installation remains separate from Servo and replaces only its own file."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('engine_build',ROOT/'scripts/build-wega-engine.py')
builder=importlib.util.module_from_spec(spec);spec.loader.exec_module(builder)


class EngineBuildTests(unittest.TestCase):
    def test_installs_worker_atomically_without_touching_browser(self):
        with tempfile.TemporaryDirectory() as temporary:
            target=Path(temporary).resolve();name='wega-engine.exe' if os.name=='nt' else 'wega-engine';source=target/'wega-engine/debug'/name;source.parent.mkdir(parents=True);source.write_bytes(b'new worker')
            browser=target/'debug/wega';browser.parent.mkdir();browser.write_bytes(b'existing browser')
            previous=browser.with_name(name);previous.write_bytes(b'old worker')
            with patch.dict(os.environ,{'CARGO_TARGET_DIR':str(target),'CARGO_BUILD_TARGET':'','CI':''}),patch.object(builder.subprocess,'run') as run:
                installed=builder.build('dev',jobs=2)
            self.assertEqual(installed.read_bytes(),b'new worker');self.assertEqual(browser.read_bytes(),b'existing browser')
            self.assertFalse(installed.with_suffix('.new').exists())
            args=run.call_args.args[0]
            self.assertIn(str(ROOT/'engines/Cargo.toml'),args)
            self.assertEqual(args[args.index('--target-dir')+1],str(target/'wega-engine'))
            self.assertTrue(run.call_args.kwargs['check'])

    def test_windows_cross_target_and_missing_artifact_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            target=Path(temporary).resolve();triple='x86_64-pc-windows-msvc'
            source=target/'wega-engine'/triple/'release/wega-engine.exe';source.parent.mkdir(parents=True);source.write_bytes(b'Windows worker')
            with patch.dict(os.environ,{'CARGO_TARGET_DIR':str(target),'CI':''}),patch.object(builder.subprocess,'run'):
                installed=builder.build('release',triple)
                self.assertEqual(installed,target/triple/'release/wega-engine.exe')
                source.unlink()
                with self.assertRaises(FileNotFoundError):builder.build('release',triple)
                self.assertEqual(installed.read_bytes(),b'Windows worker')


if __name__=='__main__':unittest.main()
