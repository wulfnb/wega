// Wega contributors, 2026. `new FormData(form)` builds the form's entry list
// natively; Deno's FormData supplies the rest of the interface.
(() => {
  const host = __wega_call;
  const Base = globalThis.FormData;
  class FormData extends Base {
    constructor(form = undefined, submitter = null) {
      super();
      if (form === undefined) return;
      if (!(form instanceof HTMLFormElement))
        throw new TypeError(
          "Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.",
        );
      for (const [name, value] of host(
        "formEntries",
        form._id,
        submitter ? submitter._id : "",
      ))
        this.append(name, value);
    }
  }
  globalThis.FormData = FormData;
})();
