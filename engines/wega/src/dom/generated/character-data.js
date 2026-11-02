// ---------------------------------------------------------------------------
// wega-webidl generated bindings - family "character-data"
// Do not edit by hand. Regenerate with:
//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate
// Pinned inputs (tools/webidl/idls.json):
//   vendor/servo-script-bindings/webidls/CharacterData.webidl sha256=e39377156a6a172c5a6e72b697d76148ff60c50e2dac364461399fd1386cd4ce
//   vendor/servo-script-bindings/webidls/Text.webidl sha256=9f2dcac227b53e8011070a8ea1b3430ba092d2915184861486a1cd8ef2bdbc48
//   vendor/servo-script-bindings/webidls/Comment.webidl sha256=1b16ed9f9e0e073f33f1f37bb723c322b98d8e2017f61b1e61be538074e3b4ea
// Hook manifest: engines/wega/src/dom/generated/character-data.hooks.json
// ---------------------------------------------------------------------------
((dom) => {
  const INTERNAL = dom.internal;
  function op(name, ...args) {
    try {
      return dom.hook(name, ...args);
    } catch (error) {
      const message = String(error && error.message !== undefined ? error.message : error);
      const cut = message.indexOf(": ");
      if (cut > 0) {
        const name = message.slice(0, cut);
        if (/^[A-Z][A-Za-z]*Error$/.test(name)) dom.fail(name, message.slice(cut + 2));
      }
      throw error;
    }
  }
  function toDomString(value) {
    return String(value);
  }
  function toLegacyNullDomString(value) {
    return value === null ? "" : String(value);
  }
  function toUnsignedLong(value) {
    let number = Number(value);
    if (Number.isNaN(number)) number = 0;
    number = Math.trunc(number);
    return ((number % 4294967296) + 4294967296) % 4294967296;
  }
  class CharacterData extends dom.base.Node {
    constructor(id, internal, info) {
      if (internal !== INTERNAL) throw new TypeError("Illegal constructor");
      super(id, nodeInfo);
      dom.brands([CharacterData], this);
    }
    get data() {
      dom.checkBrand(CharacterData, this);
      return op("charData.data", this._id);
    }
    set data(value) {
      dom.checkBrand(CharacterData, this);
      const result = op("charData.data.set", this._id, toLegacyNullDomString(value));
      dom.reactions.characterData(this, result);
    }
    get length() {
      dom.checkBrand(CharacterData, this);
      return op("charData.length", this._id);
    }
    substringData(offset, count) {
      dom.checkBrand(CharacterData, this);
      return op("charData.substringData", this._id, toUnsignedLong(offset), toUnsignedLong(count));
    }
    appendData(data) {
      dom.checkBrand(CharacterData, this);
      const result = op("charData.appendData", this._id, toDomString(data));
      dom.reactions.characterData(this, result);
    }
    insertData(offset, data) {
      dom.checkBrand(CharacterData, this);
      const result = op("charData.insertData", this._id, toUnsignedLong(offset), toDomString(data));
      dom.reactions.characterData(this, result);
    }
    deleteData(offset, count) {
      dom.checkBrand(CharacterData, this);
      const result = op("charData.deleteData", this._id, toUnsignedLong(offset), toUnsignedLong(count));
      dom.reactions.characterData(this, result);
    }
    replaceData(offset, count, data) {
      dom.checkBrand(CharacterData, this);
      const result = op("charData.replaceData", this._id, toUnsignedLong(offset), toUnsignedLong(count), toDomString(data));
      dom.reactions.characterData(this, result);
    }
  }
  class Text extends CharacterData {
    constructor(data, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = data;
        nodeInfo = info;
      } else {
        const created = op("charData.construct.text", data === undefined ? "" : toDomString(data));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, INTERNAL, nodeInfo);
      dom.brands([CharacterData, Text], this);
    }
    splitText(offset) {
      dom.checkBrand(Text, this);
      const result = op("charData.splitText", this._id, toUnsignedLong(offset));
      const next = dom.wrap(result.id, this.ownerDocument || this);
      dom.reactions.splitText(this, next, result);
      return next;
    }
    get wholeText() {
      dom.checkBrand(Text, this);
      return op("charData.wholeText", this._id);
    }
  }
  class Comment extends CharacterData {
    constructor(data, internal, info) {
      let id, nodeInfo;
      if (internal === INTERNAL) {
        id = data;
        nodeInfo = info;
      } else {
        const created = op("charData.construct.comment", data === undefined ? "" : toDomString(data));
        id = created.id;
        nodeInfo = created.info;
      }
      super(id, INTERNAL, nodeInfo);
      dom.brands([CharacterData, Comment], this);
    }
  }
  dom.classes.CharacterData = CharacterData;
  dom.classes.Text = Text;
  dom.classes.Comment = Comment;
  dom.globals.Text = Text;
  dom.globals.Comment = Comment;
})(globalThis.__wega_dom);
