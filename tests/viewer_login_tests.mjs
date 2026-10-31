import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {fileURLToPath} from 'node:url';
const filename = fileURLToPath(new URL('../docker/viewer/login.js', import.meta.url));
const source = readFileSync(filename, 'utf8');

function fixture(responses) {
  const button = {disabled: false};
  const notice = {textContent: ''};
  const calls = [];
  const locations = [];
  let submit;
  const form = {
    elements: {username: {value: 'alice'}, password: {value: 'test-password', type:'password', addEventListener(event, callback) {this[event] = callback;}}},
    querySelector: selector => { assert.equal(selector, 'button[type="submit"]'); return button; },
    addEventListener: (event, callback) => { assert.equal(event, 'submit'); submit = callback; }
  };
  const toggle = {addEventListener(event, callback) {this[event] = callback;}, setAttribute(name, value) {this[name] = value;}};
  const caps = {hidden:true};
  const context = {
    document: {querySelector: selector => selector === '#login-form' ? form : selector === '#show-password' ? toggle : selector === '#caps-warning' ? caps : notice},
    fetch: async (...args) => {
      calls.push(args);
      const response = responses.shift();
      if (response instanceof Error) throw response;
      return response;
    },
    location: {replace: value => locations.push(value)},
    Date: {now: () => 1234}
  };
  new vm.Script(source, {filename}).runInNewContext(context);
  return {button, notice, form, calls, locations, submit, toggle, caps};
}

test('login verifies the session cookie before redirect and clears the password', async () => {
  const f = fixture([{ok: true, json: async () => ({})}, {ok: true}]);
  let prevented = false;
  await f.submit({preventDefault: () => { prevented = true; }});
  assert.equal(prevented, true);
  assert.deepEqual(f.locations, ['/?signed-in=1234']);
  assert.equal(f.calls[0][0], '/auth/login');
  assert.deepEqual(JSON.parse(f.calls[0][1].body), {username: 'alice', password: 'test-password'});
  assert.equal(f.calls[0][1].credentials, 'same-origin');
  assert.equal(f.calls[1][0], '/auth/account');
  assert.equal(f.form.elements.password.value, '');
  assert.equal(f.button.disabled, false);
});

for (const [name, responses, message] of [
  ['invalid credentials', [{ok: false, json: async () => ({error: 'Incorrect credentials'})}], 'Incorrect credentials'],
  ['unexplained server rejection', [{ok: false, json: async () => ({})}], 'Could not sign in.'],
  ['missing session cookie', [{ok: true, json: async () => ({})}, {ok: false}], 'Your password was accepted'],
  ['network failure', [new Error('Network unavailable')], 'Network unavailable'],
  ['invalid JSON', [{ok: false, json: async () => {throw new Error('Invalid response');}}], 'The server sent an unexpected reply'],
  ['gateway error page', [{ok: false, status: 502, json: async () => {throw new SyntaxError("Unexpected token '<'");}}], 'The browser is starting or unavailable']
]) {
  test(name + ' never redirects and restores controls', async () => {
    const f = fixture(responses);
    await f.submit({preventDefault() {}});
    assert.ok(f.notice.textContent.startsWith(message));
    assert.deepEqual(f.locations, []);
    assert.equal(f.form.elements.password.value, '');
    assert.equal(f.button.disabled, false);
  });
}

test('password visibility and Caps Lock warning are keyboard accessible', () => {
  const f = fixture([]);
  f.toggle.click();
  assert.equal(f.form.elements.password.type, 'text');
  assert.equal(f.toggle['aria-pressed'], 'true');
  f.toggle.click();
  assert.equal(f.form.elements.password.type, 'password');
  f.form.elements.password.keydown({getModifierState: () => true});
  assert.equal(f.caps.hidden, false);
  f.form.elements.password.keyup({getModifierState: () => false});
  assert.equal(f.caps.hidden, true);
  f.form.elements.password.blur();
  assert.equal(f.caps.hidden, true);
});
for (const [status, message] of [[401, 'Username or password'], [429, 'Too many sign-in']]) {
  test(`plain language sign-in error for HTTP ${status}`, async () => {
    const f = fixture([{ok:false, status, json:async()=>{throw new Error('Proxy returned HTML');}}]);
    await f.submit({preventDefault(){}});
    assert.ok(f.notice.textContent.startsWith(message));
  });
}
