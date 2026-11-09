"use strict";

/* Loads app/static/app.js outside a browser.
 *
 * The UI is a hand-written classic script with no build step and no package
 * manager, so there is nothing to import. Instead this runs the real file in a
 * `node:vm` context whose `document`, `window` and `localStorage` only record
 * calls. Nothing is rewritten and no source is copied: the functions under test
 * are the functions the browser runs.
 *
 * At load the file registers listeners, sizes its panels from `style` custom
 * properties and starts a few async loaders; the stubs below exist for exactly
 * those, and each one is inert where a real browser would draw. Anything that
 * needs a layout — a rect, a repaint, a measured element — is deliberately
 * absent, which keeps a test from depending on rendering it never checked.
 */

const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

/** A stand-in `style`: remembers custom properties the way the panel code uses
 * them, and tolerates plain property assignment (`style.height = "auto"`). */
function stubStyle() {
  const values = new Map();
  return {
    setProperty(name, value) {
      values.set(String(name), String(value));
    },
    getPropertyValue(name) {
      return values.get(String(name)) ?? "";
    },
    removeProperty(name) {
      values.delete(String(name));
    },
    get cssText() {
      return Array.from(values.entries())
        .map(([name, value]) => `${name}: ${value};`)
        .join(" ");
    },
    set cssText(text) {
      values.clear();
    },
  };
}

/** A stand-in element: records listeners, returns itself for every property. */
function stubElement(id) {
  const element = {
    id,
    dataset: {},
    style: stubStyle(),
    classList: {
      _set: new Set(),
      add(...names) {
        names.forEach((name) => this._set.add(name));
      },
      remove(...names) {
        names.forEach((name) => this._set.delete(name));
      },
      toggle(name, force) {
        const on = force === undefined ? !this._set.has(name) : Boolean(force);
        if (on) this._set.add(name);
        else this._set.delete(name);
        return on;
      },
      contains(name) {
        return this._set.has(name);
      },
    },
    children: [],
    value: "",
    textContent: "",
    innerHTML: "",
    scrollTop: 0,
    scrollHeight: 0,
    offsetHeight: 0,
    addEventListener() {},
    removeEventListener() {},
    appendChild(child) {
      this.children.push(child);
      return child;
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
    replaceChildren(...nodes) {
      this.children = nodes;
    },
    remove() {},
    focus() {},
    blur() {},
    setAttribute() {},
    getAttribute() {
      return null;
    },
    removeAttribute() {},
    querySelector() {
      return stubElement(null);
    },
    querySelectorAll() {
      return [];
    },
    getBoundingClientRect() {
      return { top: 0, left: 0, width: 0, height: 0, bottom: 0, right: 0 };
    },
    scrollIntoView() {},
    click() {},
    contains() {
      return false;
    },
    get firstChild() {
      return null;
    },
    get lastChild() {
      return null;
    },
  };
  return element;
}

/** The smallest DOM the file touches at load time. */
function stubGlobals() {
  const listeners = [];
  const document = {
    documentElement: stubElement("html"),
    body: stubElement("body"),
    getElementById(id) {
      return stubElement(id);
    },
    querySelector() {
      return stubElement(null);
    },
    querySelectorAll() {
      return [];
    },
    createElement(tag) {
      return stubElement(tag);
    },
    createTextNode(text) {
      return { textContent: text };
    },
    addEventListener(type, handler) {
      listeners.push({ type, handler });
    },
    removeEventListener() {},
    hidden: false,
    visibilityState: "visible",
    activeElement: null,
    execCommand() {
      return true;
    },
  };

  const storage = new Map();
  const localStorage = {
    getItem(key) {
      return storage.has(String(key)) ? storage.get(String(key)) : null;
    },
    setItem(key, value) {
      storage.set(String(key), String(value));
    },
    removeItem(key) {
      storage.delete(String(key));
    },
    clear() {
      storage.clear();
    },
    key(index) {
      return Array.from(storage.keys())[index] ?? null;
    },
    get length() {
      return storage.size;
    },
  };

  const window = {
    addEventListener(type, handler) {
      listeners.push({ type, handler });
    },
    removeEventListener() {},
    matchMedia() {
      return {
        matches: false,
        addEventListener() {},
        removeEventListener() {},
      };
    },
    localStorage,
    location: { href: "http://127.0.0.1:0/", origin: "http://127.0.0.1:0" },
    // Inert on purpose: the UI schedules repaints and polling, and a live timer
    // would keep the test process running after the last assertion. Nothing
    // under test depends on time passing — `relativeTime` takes `now` as an
    // argument for exactly that reason.
    setTimeout: () => 0,
    clearTimeout: () => {},
    setInterval: () => 0,
    clearInterval: () => {},
    requestAnimationFrame: () => 0,
    cancelAnimationFrame: () => {},
    getComputedStyle() {
      return { getPropertyValue: () => "" };
    },
    scrollTo() {},
    innerWidth: 1280,
    innerHeight: 800,
    navigator: { clipboard: { writeText: async () => {} }, userAgent: "node-test" },
  };

  const context = {
    document,
    window,
    localStorage,
    navigator: window.navigator,
    console,
    setTimeout: window.setTimeout,
    clearTimeout: window.clearTimeout,
    setInterval: window.setInterval,
    clearInterval: window.clearInterval,
    requestAnimationFrame: window.requestAnimationFrame,
    cancelAnimationFrame: window.cancelAnimationFrame,
    // A refusal, not a crash: the load-time loaders are written to handle a
    // failed response, and rejecting the promise here would surface as an
    // unhandled rejection in a test that never asked for a request.
    fetch: async () => ({
      ok: false,
      status: 503,
      json: async () => ({ error: "the test harness has no server" }),
    }),
    URL,
    URLSearchParams,
    Blob: class Blob {
      constructor(parts) {
        this.parts = parts;
      }
    },
    ResizeObserver: class ResizeObserver {
      observe() {}
      disconnect() {}
    },
    // Present but inert: the script uses these to mirror attributes and
    // visibility, which nothing here renders, so a recorded callback is the
    // most a browser-free harness can honestly offer.
    MutationObserver: class MutationObserver {
      constructor(callback) {
        this.callback = callback;
      }
      observe() {}
      disconnect() {}
      takeRecords() {
        return [];
      }
    },
    IntersectionObserver: class IntersectionObserver {
      constructor(callback) {
        this.callback = callback;
      }
      observe() {}
      disconnect() {}
      takeRecords() {
        return [];
      }
    },
    EventSource: class EventSource {
      constructor() {
        this.readyState = 0;
      }
      addEventListener() {}
      close() {}
    },
    CustomEvent: class CustomEvent {
      constructor(type, init) {
        this.type = type;
        this.detail = init && init.detail;
      }
    },
    Date,
    Math,
    JSON,
    Object,
    Array,
    String,
    Number,
    Boolean,
    Map,
    Set,
    Promise,
    Error,
    RegExp,
    Intl,
    isNaN,
    parseInt,
    parseFloat,
    encodeURIComponent,
    decodeURIComponent,
    structuredClone: globalThis.structuredClone,
    // A few globals the file reads without guarding; undefined would throw on
    // property access rather than on use.
    HTMLElement: class HTMLElement {},
    Node: class Node {},
    listeners,
  };
  context.globalThis = context;
  return context;
}

/** The real UI script, evaluated in a stub context. */
function loadApp() {
  const file = path.join(__dirname, "..", "app.js");
  const source = fs.readFileSync(file, "utf8");
  const context = vm.createContext(stubGlobals());
  vm.runInContext(source, context, { filename: file });
  return context;
}

/** Call one of the script's functions or read one of its top-level bindings. */
function evaluate(context, expression) {
  return vm.runInContext(expression, context);
}

module.exports = { loadApp, evaluate };
