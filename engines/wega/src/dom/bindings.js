// Wega contributors, 2026. Typed WebIDL binding runtime; semantic state is private.
(() => {
  const NativeWeakMap = WeakMap, NativeWeakSet = WeakSet;
  const apply = Reflect.apply, freeze = Object.freeze;
  const weakGet = WeakMap.prototype.get, weakSet = WeakMap.prototype.set, weakHas = WeakMap.prototype.has;
  const weakSetAdd = WeakSet.prototype.add, weakSetHas = WeakSet.prototype.has;
  const isPrototypeOf = Object.prototype.isPrototypeOf;
  function privateWeakMap() {
    const map = new NativeWeakMap();
    return freeze({ get: key => apply(weakGet, map, [key]), set: (key, value) => apply(weakSet, map, [key, value]), has: key => apply(weakHas, map, [key]) });
  }
  function privateWeakSet() {
    const set = new NativeWeakSet();
    return freeze({ add: value => apply(weakSetAdd, set, [value]), has: value => apply(weakSetHas, set, [value]) });
  }
  const semantics = globalThis.__wega_semantics;
  const guard = globalThis.__wega_guard_binding;
  const installed = new Set();
  const has = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
  const objectLike = value => value !== null && (typeof value === "object" || typeof value === "function");
  function string(value) {
    if (typeof value === "symbol") throw new TypeError("Cannot convert a Symbol to DOMString");
    return String(value);
  }
  function number(value) {
    if (typeof value === "bigint") throw new TypeError("Cannot convert BigInt to a number");
    return +value;
  }
  function convert(type, value, ids, depth = 0) {
    if (depth > 32) throw new TypeError("WebIDL value exceeds nesting limit");
    if (type.nullable && value == null) return null;
    switch (type.kind) {
      case "any": return value;
      case "undefined": return undefined;
      case "string": return string(value);
      case "boolean": return !!value;
      case "unsigned": {
        const valueNumber = number(value), modulus = 2 ** type.bits;
        return !Number.isFinite(valueNumber) || valueNumber === 0 ? 0 : ((Math.trunc(valueNumber) % modulus) + modulus) % modulus;
      }
      case "float": case "double": {
        const result = type.kind === "float" ? Math.fround(number(value)) : number(value);
        if (!Number.isFinite(result)) throw new TypeError("Non-finite floating-point value");
        return result;
      }
      case "object": if (!objectLike(value)) throw new TypeError("Expected an object"); return value;
      case "callback-interface": if (!objectLike(value)) throw new TypeError(`Expected ${type.name} listener`); return value;
      case "callback": if (typeof value !== "function") throw new TypeError(`Expected ${type.name} callback`); return value;
      case "interface": {
        if (!semantics.brand(type.name, value)) throw new TypeError(`Expected ${type.name}`);
        ids.push(...semantics.ids(value)); return value;
      }
      case "enum": {
        const result = string(value);
        if (!type.values.includes(result)) throw new TypeError(`Invalid ${type.name} value '${result}'`);
        return result;
      }
      case "dictionary": {
        if (value != null && !objectLike(value)) throw new TypeError(`Expected ${type.name} dictionary`);
        const result = Object.create(null);
        // Members are emitted in ancestor order, then Unicode code-point order.
        for (const member of type.members) {
          let entry = value == null ? undefined : value[member.name];
          if (entry === undefined && has(member, "default")) entry = member.default;
          if (entry === undefined) {
            if (!member.optional) throw new TypeError(`Required dictionary member '${member.name}' is missing`);
            continue;
          }
          result[member.name] = convert(member.type, entry, ids, depth + 1);
        }
        return result;
      }
      case "sequence": {
        if (!objectLike(value)) throw new TypeError("Expected an iterable object");
        const result = [];
        for (const entry of value) {
          if (result.length >= 65536) throw new TypeError("WebIDL sequence limit exceeded");
          result.push(convert(type.element, entry, ids, depth + 1));
        }
        return result;
      }
      case "union": {
        const types = type.types;
        const nullable = types.find(t => t.nullable);
        if (value == null && nullable) return convert(nullable, value, ids, depth + 1);
        if (objectLike(value)) {
          const interfaceType = types.find(t => t.kind === "interface" && semantics.brand(t.name, value));
          if (interfaceType) return convert(interfaceType, value, ids, depth + 1);
          const callbackType = types.find(t => t.kind === "callback");
          if (typeof value === "function" && callbackType) return convert(callbackType, value, ids, depth + 1);
          const sequence = types.find(t => t.kind === "sequence");
          if (sequence && value[Symbol.iterator] != null) return convert(sequence, value, ids, depth + 1);
          const dictionary = types.find(t => t.kind === "dictionary");
          if (dictionary) return convert(dictionary, value, ids, depth + 1);
          const object = types.find(t => t.kind === "object");
          if (object) return value;
        }
        const dictionary = types.find(t => t.kind === "dictionary");
        if (value == null && dictionary) return convert(dictionary, value, ids, depth + 1);
        const boolean = types.find(t => t.kind === "boolean");
        if (typeof value === "boolean" && boolean) return value;
        const numeric = types.find(t => ["unsigned", "float", "double"].includes(t.kind));
        if (typeof value === "number" && numeric) return convert(numeric, value, ids, depth + 1);
        const text = types.find(t => t.kind === "string" || t.kind === "enum");
        if (text) return convert(text, value, ids, depth + 1);
        if (numeric) return convert(numeric, value, ids, depth + 1);
        if (boolean) return !!value;
        throw new TypeError("Value matches no WebIDL union alternative");
      }
      default: throw new TypeError(`Unknown generated type ${type.kind}`);
    }
  }
  function argumentsFor(parameters, args, ids) {
    const required = parameters.filter(p => !p.optional && !p.variadic).length;
    if (args.length < required) throw new TypeError(`Expected at least ${required} arguments`);
    const converted = [];
    for (let index = 0; index < parameters.length; index++) {
      const parameter = parameters[index];
      if (parameter.variadic) {
        for (let at = index; at < args.length; at++) converted.push(convert(parameter.type, args[at], ids));
        break;
      }
      let value = args[index];
      if (value === undefined && has(parameter, "default")) value = parameter.default;
      if (value === undefined && parameter.optional && !has(parameter, "default")) converted.push(undefined);
      else converted.push(convert(parameter.type, value, ids));
    }
    return converted;
  }
  function descriptorFor(Ctor, name, isStatic) {
    for (let prototype = isStatic ? Ctor : Ctor.prototype; prototype; prototype = Object.getPrototypeOf(prototype)) {
      const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
      if (descriptor) return descriptor;
    }
    throw new Error(`Missing semantic hook ${Ctor.name}.${name}`);
  }
  function shape(fn, name, parameters) {
    Object.defineProperty(fn, "name", { value: name, configurable: true });
    Object.defineProperty(fn, "length", { value: parameters.findIndex(p => p.optional || p.variadic) < 0 ? parameters.length : parameters.findIndex(p => p.optional || p.variadic), configurable: true });
    return fn;
  }
  function checkReceiver(name, receiver) {
    if (!semantics.brand(name, receiver)) throw new TypeError(`Illegal invocation of ${name}`);
    return semantics.ids(receiver);
  }
  // [LegacyUnforgeable] members of upstream classes (Deno Event.isTrusted) are
  // non-configurable and already carry their own semantics; keep them as-is.
  function upstreamLocked(target, name) {
    const existing = Object.getOwnPropertyDescriptor(target, name);
    return !!existing && !existing.configurable;
  }
  function install(spec) {
    if (installed.has(spec.name)) throw new Error(`Duplicate binding interface ${spec.name}`);
    const Ctor = semantics.classes[spec.name];
    if (!Ctor) throw new Error(`No semantic adapter for ${spec.name}`);
    const constructors = spec.members.filter(member => member.kind === "constructor");
    for (const member of spec.members) {
      const name = member.name;
      if (member.kind === "constant") {
        // WebIDL constants are non-configurable; upstream classes (Deno Event) may already own them.
        for (const target of [Ctor, Ctor.prototype]) {
          const existing = Object.getOwnPropertyDescriptor(target, name);
          if (existing && !existing.configurable) {
            if (existing.value !== member.value) throw new Error(`Conflicting constant ${spec.name}.${name}`);
            continue;
          }
          Object.defineProperty(target, name, { value: member.value, writable: false, enumerable: true, configurable: false });
        }
      } else if (member.kind === "enum") {
        Object.defineProperty(Ctor, name, { value: Object.freeze(Object.fromEntries(member.values.map(v => [v, v]))), configurable: true });
      } else if (member.kind === "attribute") {
        if (upstreamLocked(Ctor.prototype, name)) continue;
        const original = descriptorFor(Ctor, name, false), cached = privateWeakMap();
        if (typeof original.get !== "function") throw new Error(`No getter for ${spec.name}.${name}`);
        const get = function () {
          const ids = checkReceiver(spec.name, this);
          guard(`${spec.prefix}.${name}`, ids);
          if (member.sameObject && cached.has(this)) return cached.get(this);
          const value = Reflect.apply(original.get, this, []);
          if (member.sameObject) cached.set(this, value);
          return value;
        };
        const descriptor = { get: shape(get, `get ${name}`, []), configurable: true, enumerable: true };
        if (!member.readonly || member.forwards) {
          descriptor.set = shape(function (value) {
            const ids = checkReceiver(spec.name, this);
            if (member.forwards) {
              guard(`${spec.prefix}.${name}.set`, ids);
              const target = Reflect.apply(original.get, this, []);
              if (!target) throw new TypeError("PutForwards target is null");
              target[member.forwards] = value;
              return;
            }
            const converted = convert(member.type, member.legacyNull && value === null ? "" : value, ids);
            guard(`${spec.prefix}.${name}.set`, ids);
            Reflect.apply(original.set, this, [converted]);
          }, `set ${name}`, [{ optional: false }]);
          if (!member.forwards && typeof original.set !== "function") throw new Error(`No setter for ${spec.name}.${name}`);
        }
        Object.defineProperty(Ctor.prototype, name, descriptor);
      } else if (member.kind === "operation") {
        if (upstreamLocked(member.static ? Ctor : Ctor.prototype, name)) continue;
        const original = descriptorFor(Ctor, name, member.static).value;
        if (typeof original !== "function") throw new Error(`No operation for ${spec.name}.${name}`);
        const cache = privateWeakMap();
        const method = function (...args) {
          const ids = member.static ? [] : checkReceiver(spec.name, this);
          const values = argumentsFor(member.parameters, args, ids);
          guard(`${spec.prefix}.${name}`, ids);
          // SameObject operations memoize per receiver AND converted argument tuple.
          let leaf;
          if (member.sameObject) {
            leaf = cache.get(this); if (!leaf) cache.set(this, leaf = new Map());
            for (const value of values) { let next = leaf.get(value); if (!next) leaf.set(value, next = new Map()); leaf = next; }
            if (leaf.has(internalResult)) return leaf.get(internalResult);
          }
          const result = Reflect.apply(original, this, values);
          if (leaf) leaf.set(internalResult, result);
          return result;
        };
        Object.defineProperty(member.static ? Ctor : Ctor.prototype, name, { value: shape(method, name, member.parameters), writable: true, configurable: true, enumerable: true });
      } else if (member.kind === "iterable" && member.types.length === 1) {
        // WebIDL value iterators on indexed interfaces use %Array.prototype% methods.
        const prototype = Ctor.prototype;
        for (const kind of ["keys", "values", "entries", "forEach"]) Object.defineProperty(prototype, kind, { configurable: true, writable: true, enumerable: true, value: Array.prototype[kind] });
        Object.defineProperty(prototype, Symbol.iterator, { value: Array.prototype.values, configurable: true, writable: true });
      } else if (member.kind === "iterable") {
        const prototype = Ctor.prototype;
        // Indexed collection algorithms read the live length at every iteration.
        for (const kind of ["keys", "values", "entries"]) Object.defineProperty(prototype, kind, { configurable: true, writable: true, value: function* () {
          checkReceiver(spec.name, this);
          for (let index = 0; index < this.length; index++) yield kind === "keys" ? index : kind === "entries" ? [index, this.item(index)] : this.item(index);
        }});
        Object.defineProperty(prototype, "forEach", { configurable: true, writable: true, value: function(callback, thisArg) {
          checkReceiver(spec.name, this);
          if (typeof callback !== "function") throw new TypeError("Callback required");
          for (let index = 0; index < this.length; index++) callback.call(thisArg, this.item(index), index, this);
        }});
        Object.defineProperty(prototype, Symbol.iterator, { value: prototype.values, configurable: true, writable: true });
      }
    }
    const tag = Object.getOwnPropertyDescriptor(Ctor.prototype, Symbol.toStringTag);
    if (!tag || tag.configurable) Object.defineProperty(Ctor.prototype, Symbol.toStringTag, { value: spec.name, configurable: true });
    if (spec.exposed) {
      const facade = new Proxy(Ctor, { construct(_target, args, newTarget) {
        const constructor = constructors[0];
        if (!constructor) throw new TypeError("Illegal constructor");
        const ids = [], values = argumentsFor(constructor.parameters, args, ids);
        guard(`${spec.prefix}.construct.${spec.name.toLowerCase()}`, ids);
        return semantics.construct(spec.name, values, newTarget);
      }});
      Object.defineProperty(Ctor.prototype, "constructor", { value: facade, configurable: true, writable: true });
      globalThis[spec.name] = facade;
    }
    installed.add(spec.name);
  }
  const internalResult = Symbol("same-object operation result");
  Object.defineProperty(globalThis, "__wega_dom", { configurable: true, value: { install } });
})();
