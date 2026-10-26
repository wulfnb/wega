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
 # V8 isolates at both the callback and native poll-error boundary.
 ('timer-isolation',
  [('engines/wega/src/dom/web.js',
    '        __wega_report(error);',
    '        throw error;'),
   ('engines/wega/src/v8.rs',
    'fatal: self.oom.load(Ordering::Relaxed) || timed_out,',
    'fatal: true,')],
  'one_timer_exception_does_not_block_other_timers'),
 ('missing-api-tracker',
  [('engines/wega/src/dom/bindings.js',
    'missing.add((info.nodeType === 9 ? "Document." : "Element.") + key);',
    'void 0;')],
  'missing_api_names_source_and_keeps_the_next_script_alive'),
 ('source-diagnostics',
  [('engines/wega/src/view.rs',
    'format!("{document} inline #{}", self.script_count)',
    'String::from("anonymous")')],
  'missing_api_names_source_and_keeps_the_next_script_alive'),
 ('window-onerror',
  [('engines/wega/src/dom/bindings.js',
    '      web.event.defineEventHandler(',
    '      if (name !== "error" || target !== globalThis) web.event.defineEventHandler(')],
  'script_errors_fire_onerror_and_error_event'),
 ('error-event',
  [('engines/wega/src/dom/bindings.js', 'globalThis.dispatchEvent(event);', 'void 0;')],
  'script_errors_fire_onerror_and_error_event'),
 ('evaluation-diagnostics',
  [('engines/wega/src/v8.rs',
    'message: format!("{name}: {text}"),',
    'message: format!("{name}: JavaScript exception"),')],
  'evaluate_json_preserves_exception_text'),
 ('fatal-teardown',
  [('engines/wega/src/view.rs', 'if error.fatal {', 'if false {')],
  'deadline_remains_fatal_and_a_new_document_recovers'),
 ('heap-and-stack-classification',
  [('engines/wega/src/v8.rs',
    'fatal: text.contains("Maximum call stack size exceeded"),',
    'fatal: false,')],
  'heap_and_stack_exhaustion_remain_fatal')]
    results=[]
    for name,patches,test in mutations:
        originals={ROOT/relative:(ROOT/relative).read_text() for relative,_,_ in patches}
        try:
            for relative,before,after in patches:
                path=ROOT/relative;source=path.read_text()
                if before not in source:raise RuntimeError(f'{name}: mutation anchor missing')
                path.write_text(source.replace(before,after,1))
            run=subprocess.run(['cargo','test','--locked','--manifest-path',str(ROOT/'engines/Cargo.toml'),'--target-dir',args.target_dir,'--jobs','2','--test','errors',test,'--','--exact'],cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
            detected=run.returncode!=0 and f'test {test} ... FAILED' in run.stdout and '1 failed' in run.stdout
            results.append({'mutation':name,'test':test,'detected':detected})
            print(f'{name}: {"DETECTED" if detected else "FAILED TO PROVE"}',flush=True)
            if not detected:print(run.stdout);raise RuntimeError(name)
        finally:
            for path,source in originals.items():path.write_text(source)
    output=Path(args.output);output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(results,indent=2)+'\n')
    # Rebuild original sources after the final mutation; mutant artifacts are never used for a gate.
    subprocess.run(['cargo','test','--locked','--manifest-path',str(ROOT/'engines/Cargo.toml'),'--target-dir',args.target_dir,'--jobs','2'],cwd=ROOT,check=True)

if __name__=='__main__':main()
