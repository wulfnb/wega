#!/usr/bin/env python3
"""Prove P0 regression contracts by breaking one rule, running its test, then restoring.

Never interrupts restoration on an ordinary test/build failure. Use a disposable target
or pass --target-dir to reuse a target already built from clean sources.
"""
import argparse
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--target-dir',default=str(ROOT/'target/p0-mutations'))
    parser.add_argument('--output',default=str(ROOT/'target/p0-mutations.json'))
    args=parser.parse_args()
    mutations=[('ordinary-error-isolation',
  [('engines/wega/src/view.rs', 'if error.fatal {', 'if true {')],
  'missing_api_names_source_and_keeps_the_next_script_alive'),
 ('listener-isolation',
  [('engines/wega/src/dom/bindings.js',
    '        try {\n'
    '          typeof callback === "function"\n'
    '            ? callback.call(this, event)\n'
    '            : callback.handleEvent(event);\n'
    '        } catch (error) {\n'
    '          report(error);\n'
    '        }',
    '        try {\n'
    '          typeof callback === "function"\n'
    '            ? callback.call(this, event)\n'
    '            : callback.handleEvent(event);\n'
    '        } catch (error) {\n'
    '          throw error;\n'
    '        }'),
   ('engines/wega/src/dom/bindings.js',
    '          try {\n'
    '            typeof entry.callback === "function"\n'
    '              ? entry.callback.call(target, event)\n'
    '              : entry.callback.handleEvent(event);\n'
    '          } catch (error) {\n'
    '            report(error);\n'
    '          }',
    '          try {\n'
    '            typeof entry.callback === "function"\n'
    '              ? entry.callback.call(target, event)\n'
    '              : entry.callback.handleEvent(event);\n'
    '          } catch (error) {\n'
    '            throw error;\n'
    '          }')],
  'one_listener_exception_does_not_block_other_listeners'),
 ('timer-isolation',
  [('engines/wega/src/dom/bindings.js',
    '          try {\n'
    '            typeof timer.fn === "function"\n'
    '              ? timer.fn(...timer.args)\n'
    '              : (0, eval)(String(timer.fn));\n'
    '          } catch (error) {\n'
    '            report(error);\n'
    '          }',
    '          try {\n'
    '            typeof timer.fn === "function"\n'
    '              ? timer.fn(...timer.args)\n'
    '              : (0, eval)(String(timer.fn));\n'
    '          } catch (error) {\n'
    '            throw error;\n'
    '          }')],
  'one_timer_exception_does_not_block_other_timers'),
 ('missing-api-tracker',
  [('engines/wega/src/dom/bindings.js',
    'missing.add((info.nodeType === 9 ? "Document." : "Element.") + key);',
    'void 0;')],
  'missing_api_names_source_and_keeps_the_next_script_alive'),
 ('source-diagnostics',
  [('engines/wega/src/view.rs',
    'format!("{} inline #{}", self.url, index + 1)',
    'String::from("anonymous")')],
  'missing_api_names_source_and_keeps_the_next_script_alive'),
 ('window-onerror',
  [('engines/wega/src/dom/bindings.js',
    'entry.handler.call(\n'
    '                    this,\n'
    '                    event.message,\n'
    '                    "",\n'
    '                    0,\n'
    '                    0,\n'
    '                    event.error,\n'
    '                  )',
    'undefined')],
  'script_errors_fire_onerror_and_error_event'),
 ('error-event',
  [('engines/wega/src/dom/bindings.js', 'globalThis.dispatchEvent(event);', 'void 0;')],
  'script_errors_fire_onerror_and_error_event'),
 ('evaluation-diagnostics',
  [('engines/wega/src/quickjs.rs',
    'self.error(&ctx, e, "evaluate_json")',
    'ScriptError { fatal:false, message:e.to_string() }')],
  'evaluate_json_preserves_exception_text'),
 ('fatal-teardown',
  [('engines/wega/src/view.rs', 'if error.fatal {', 'if false {')],
  'deadline_remains_fatal_and_a_new_document_recovers'),
 ('heap-and-stack-classification',
  [('engines/wega/src/quickjs.rs',
    'ScriptError {\n'
    '            fatal,\n'
    '            message: format!("{source}: {message}"),\n'
    '        }',
    'ScriptError {\n'
    '            fatal:false,\n'
    '            message: format!("{source}: {message}"),\n'
    '        }')],
  'heap_and_stack_exhaustion_remain_fatal')]
    results=[]
    for name,patches,test in mutations:
        originals={ROOT/relative:(ROOT/relative).read_text() for relative,_,_ in patches}
        try:
            for relative,before,after in patches:
                path=ROOT/relative;source=path.read_text()
                if before not in source:raise RuntimeError(f'{name}: mutation anchor missing')
                path.write_text(source.replace(before,after,1))
            run=subprocess.run(['cargo','test','--locked','--manifest-path',str(ROOT/'engines/Cargo.toml'),'--target-dir',args.target_dir,'--jobs','2','--no-default-features','--features','quickjs','--test','errors',test,'--','--exact'],cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
            detected=run.returncode!=0 and f'test {test} ... FAILED' in run.stdout and '1 failed' in run.stdout
            results.append({'mutation':name,'test':test,'detected':detected})
            print(f'{name}: {"DETECTED" if detected else "FAILED TO PROVE"}',flush=True)
            if not detected:print(run.stdout);raise RuntimeError(name)
        finally:
            for path,source in originals.items():path.write_text(source)
    output=Path(args.output);output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(results,indent=2)+'\n')
    # Rebuild original sources after the final mutation; mutant artifacts are never used for a gate.
    subprocess.run(['cargo','test','--locked','--manifest-path',str(ROOT/'engines/Cargo.toml'),'--target-dir',args.target_dir,'--jobs','2','--no-default-features','--features','quickjs'],cwd=ROOT,check=True)

if __name__=='__main__':main()
