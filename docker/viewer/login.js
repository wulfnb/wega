// A proxy or gateway error page is HTML, not JSON: explain it instead of
// showing a parse error such as "Unexpected token '<'".
async function readJson(response) {
  try {
    return await response.json();
  } catch {
    throw new Error(response.status >= 500
      ? 'The browser is starting or unavailable. Wait a moment and try again.'
      : 'The server sent an unexpected reply. Reload the page and try again.');
  }
}
const form = document.querySelector('#login-form');
form.addEventListener('submit', async event => {
  event.preventDefault();
  const button = form.querySelector('button[type="submit"]');
  const notice = document.querySelector('#login-notice');
  button.disabled = true;
  notice.textContent = 'Signing in…';
  try {
    const response = await fetch('/auth/login', {
      method: 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({username: form.elements.username.value, password: form.elements.password.value})
    });
    if (response.status === 429) throw new Error('Too many sign-in attempts. Wait a minute and try again.');
    if (response.status === 401) throw new Error('Username or password is incorrect. Try again.');
    const result = await readJson(response);
    if (!response.ok) throw new Error(result.error || 'Could not sign in.');
    const session = await fetch('/auth/account', {credentials: 'same-origin', cache: 'no-store'});
    if (!session.ok) {
      throw new Error('Your password was accepted, but the session cookie was not saved. Allow cookies for this HTTPS address and try again.');
    }
    // Bypass login documents cached by an earlier version of the viewer.
    location.replace(`/?signed-in=${Date.now()}`);
  } catch (error) { notice.textContent = error.message; }
  finally { form.elements.password.value = ''; button.disabled = false; }
});

const password = form.elements.password;
const showPassword = document.querySelector('#show-password');
showPassword.addEventListener('click', () => {
  const visible = password.type === 'password';
  password.type = visible ? 'text' : 'password';
  showPassword.textContent = visible ? 'Hide password' : 'Show password';
  showPassword.setAttribute('aria-pressed', String(visible));
});
for (const event of ['keydown', 'keyup']) {
  password.addEventListener(event, event => {
    document.querySelector('#caps-warning').hidden = !event.getModifierState('CapsLock');
  });
}
password.addEventListener('blur', () => { document.querySelector('#caps-warning').hidden = true; });
