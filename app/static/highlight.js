/* Syntax highlighting for the editor's text overlay.
 *
 * A textarea cannot colour its own text, so the editor draws the same buffer in
 * a <pre> behind a transparent textarea. This file only turns text into escaped
 * HTML: it never touches the DOM, so the browser-free harness can test every
 * rule. Anything it does not recognise — an unknown language, a buffer over the
 * size cap — comes back escaped and uncoloured, never as raw markup.
 *
 * Grammars are line-oriented scans, not parsers: enough to read code by colour,
 * with no claim beyond that. Bold and italic inside Markdown are deliberately
 * left plain.
 */
"use strict";

/* Retokenising a buffer on every keystroke costs more than the colour is worth
   past this size, so a large file is shown uncoloured instead of lagging. */
const HIGHLIGHT_LIMIT = 200000;

const RUST_KEYWORDS =
  "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while".split(
    " ",
  );
const JS_KEYWORDS =
  "async await break case catch class const continue debugger default delete do else export extends false finally for from function get if import in instanceof let new null of return set static super switch this throw true try typeof undefined var void while with yield".split(
    " ",
  );
const TS_KEYWORDS = JS_KEYWORDS.concat(
  "abstract as declare enum implements interface keyof namespace private protected public readonly satisfies type infer is".split(
    " ",
  ),
);
const PY_KEYWORDS =
  "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield".split(
    " ",
  );
const GO_KEYWORDS =
  "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var nil true false".split(
    " ",
  );
const JAVA_KEYWORDS =
  "abstract assert break case catch class const continue default do else enum extends final finally for goto if implements import instanceof interface native new package private protected public return static strictfp super switch synchronized this throw throws transient try volatile while true false null var record".split(
    " ",
  );
const KOTLIN_KEYWORDS =
  "as break class continue do else false for fun if in interface is null object package return super this throw true try typealias val var when while".split(
    " ",
  );
const C_KEYWORDS =
  "auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while true false NULL".split(
    " ",
  );
const CSHARP_KEYWORDS =
  "abstract as async await base bool break byte case catch char class const continue decimal default delegate do double else enum event explicit extern false finally fixed float for foreach get if implicit in int interface internal is lock long namespace new null object operator out override params private protected public readonly record ref return sbyte sealed set short sizeof static string struct switch this throw true try typeof uint ulong unchecked unsafe ushort using var virtual void volatile while".split(
    " ",
  );
const RUBY_KEYWORDS =
  "alias and begin break case class def defined do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield".split(
    " ",
  );
const PHP_KEYWORDS =
  "abstract and array as break case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends false final finally fn for foreach function global if implements include instanceof interface isset list namespace new null or print private protected public require return static switch throw trait true try unset use var while".split(
    " ",
  );
const SWIFT_KEYWORDS =
  "as associatedtype break case catch class continue default defer deinit do else enum extension fallthrough false fileprivate for func guard if import in init inout internal is let nil open operator private protocol public repeat rethrows return self static struct subscript super switch throw throws true try typealias var where while".split(
    " ",
  );
const BASH_KEYWORDS =
  "case do done elif else esac fi for function if in select then time until while export local return readonly declare true false".split(
    " ",
  );
const SQL_KEYWORDS =
  "add all alter and as asc between by case cast column commit constraint create cross database default delete desc distinct drop else end exists from full group having if in index inner insert into is join key left like limit not null offset on or order outer primary references right rollback select set table then union unique update values view when where with".split(
    " ",
  );

const RUST_TYPES =
  "bool char f32 f64 i8 i16 i32 i64 i128 isize str String u8 u16 u32 u64 u128 usize Vec Option Result Box Self".split(
    " ",
  );
const GO_TYPES = "bool byte error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any".split(
  " ",
);

/* One grammar per language name the backend reports, plus aliases. */
const GRAMMARS = {
  rust: {
    keywords: RUST_KEYWORDS,
    types: RUST_TYPES,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"'],
    charLiterals: true,
  },
  javascript: {
    keywords: JS_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'", "`"],
    multiline: "`",
  },
  typescript: {
    keywords: TS_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'", "`"],
    multiline: "`",
    capitalized: true,
  },
  json: {
    keywords: ["true", "false", "null"],
    quotes: ['"'],
    keys: true,
  },
  python: {
    keywords: PY_KEYWORDS,
    line: ["#"],
    quotes: ['"', "'"],
    triples: ['"', "'"],
    multiline: true,
  },
  go: {
    keywords: GO_KEYWORDS,
    types: GO_TYPES,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "`"],
    multiline: "`",
  },
  java: {
    keywords: JAVA_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    capitalized: true,
  },
  kotlin: {
    keywords: KOTLIN_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    multiline: true,
    capitalized: true,
  },
  c: {
    keywords: C_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    charLiterals: true,
  },
  cpp: {
    keywords: C_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    charLiterals: true,
    capitalized: true,
  },
  csharp: {
    keywords: CSHARP_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    capitalized: true,
  },
  ruby: {
    keywords: RUBY_KEYWORDS,
    line: ["#"],
    quotes: ['"', "'"],
    multiline: true,
    capitalized: true,
  },
  php: {
    keywords: PHP_KEYWORDS,
    line: ["//", "#"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    multiline: true,
    capitalized: true,
  },
  swift: {
    keywords: SWIFT_KEYWORDS,
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"'],
    multiline: true,
    capitalized: true,
  },
  bash: {
    keywords: BASH_KEYWORDS,
    line: ["#"],
    quotes: ['"', "'"],
    multiline: true,
  },
  sql: {
    keywords: SQL_KEYWORDS,
    line: ["--"],
    block: [["/*", "*/"]],
    quotes: ["'"],
    capitalized: true,
    caseInsensitive: true,
  },
  yaml: {
    keywords: ["true", "false", "null", "yes", "no", "on", "off"],
    line: ["#"],
    quotes: ['"', "'"],
    keys: true,
  },
  toml: {
    keywords: ["true", "false"],
    line: ["#"],
    quotes: ['"', "'"],
    keys: true,
  },
  css: {
    line: [],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    keys: true,
    numbers: "hex",
  },
  scss: {
    line: ["//"],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    keys: true,
    numbers: "hex",
  },
};

const ALIASES = {
  rs: "rust",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  ts: "typescript",
  tsx: "typescript",
  py: "python",
  sh: "bash",
  zsh: "bash",
  shell: "bash",
  yml: "yaml",
  md: "markdown",
  htm: "html",
  xml: "html",
  html: "html",
};

function escapeCode(text) {
  return String(text === null || text === undefined ? "" : text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** The grammar key for a language name, or "" when this file cannot colour it. */
function highlightGrammar(language) {
  const key = String(language || "")
    .trim()
    .toLowerCase();
  if (key === "markdown") return "markdown";
  if (key === "html" || key === "xml") return "html";
  const resolved = ALIASES[key] || key;
  return GRAMMARS[resolved] ? resolved : "";
}

/** True when `highlightCode` will colour this language at all. */
function highlightSupported(language) {
  return highlightGrammar(language) !== "";
}

/* End of a quoted string starting at `start`; an unterminated single-line string
   stops at the newline so the rest of the line stays readable. */
function scanString(text, start, quote, grammar) {
  const triple = grammar.triples && text.startsWith(quote.repeat(3), start);
  const close = triple ? quote.repeat(3) : quote;
  const multiline =
    triple || grammar.multiline === true || grammar.multiline === quote;
  let index = start + close.length;
  while (index < text.length) {
    const ch = text[index];
    if (ch === "\\") {
      index += 2;
      continue;
    }
    if (text.startsWith(close, index)) return index + close.length;
    if (ch === "\n" && !multiline) return index;
    index += 1;
  }
  return text.length;
}

/* A Rust `'a'` is a character; `&'a str` is a lifetime. Only the first is a
   string token. */
function isCharLiteral(text, start) {
  const match = /^'(?:\\.|[^'\\\n])'/.exec(text.slice(start, start + 6));
  return Boolean(match);
}

function tokenizeCode(text, grammar) {
  const out = [];
  let plain = "";
  const flush = () => {
    if (plain) {
      out.push(escapeCode(plain));
      plain = "";
    }
  };
  const span = (cls, value) => {
    flush();
    out.push(`<span class="${cls}">${escapeCode(value)}</span>`);
  };
  const keywords = grammar.keywords || [];
  const types = grammar.types || [];
  let index = 0;
  while (index < text.length) {
    const ch = text[index];
    if (/\s/.test(ch)) {
      plain += ch;
      index += 1;
      continue;
    }
    const block = (grammar.block || []).find(([open]) => text.startsWith(open, index));
    if (block) {
      const closeAt = text.indexOf(block[1], index + block[0].length);
      const stop = closeAt === -1 ? text.length : closeAt + block[1].length;
      span("tok-com", text.slice(index, stop));
      index = stop;
      continue;
    }
    const linePrefix = (grammar.line || []).find((prefix) =>
      text.startsWith(prefix, index),
    );
    if (linePrefix) {
      const end = text.indexOf("\n", index);
      const stop = end === -1 ? text.length : end;
      span("tok-com", text.slice(index, stop));
      index = stop;
      continue;
    }
    if ((grammar.quotes || []).includes(ch) || (ch === "'" && grammar.charLiterals)) {
      // Rust and C list `'` for characters, not strings: a lifetime (`&'a str`)
      // reaches here too and must stay plain.
      const literal = ch === "'" && grammar.charLiterals ? isCharLiteral(text, index) : true;
      if (literal) {
        const stop = scanString(text, index, ch, grammar);
        const value = text.slice(index, stop);
        // In JSON, YAML and CSS a quoted key is a property; a general string is a
        // value. A trailing colon decides which.
        let cls = "tok-str";
        if (grammar.keys && /^\s*:/.test(text.slice(stop))) cls = "tok-prop";
        span(cls, value);
        index = stop;
        continue;
      }
    }
    if (ch === "#" && grammar.numbers === "hex") {
      const hex = /^#[0-9a-fA-F]{3,8}\b/.exec(text.slice(index));
      if (hex) {
        span("tok-num", hex[0]);
        index += hex[0].length;
        continue;
      }
    }
    if (/[0-9]/.test(ch) || (ch === "." && /[0-9]/.test(text[index + 1] || ""))) {
      const number = /^(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0o[0-7_]+|[0-9][0-9_]*(?:\.[0-9_]+)?(?:[eE][+-]?[0-9]+)?)/.exec(
        text.slice(index),
      );
      if (number) {
        span("tok-num", number[0]);
        index += number[0].length;
        continue;
      }
    }
    const word = /^[A-Za-z_$][A-Za-z0-9_$]*/.exec(text.slice(index));
    if (word) {
      const value = word[0];
      const after = index + value.length;
      const next = /^\s*/.exec(text.slice(after));
      const nextChar = text[after + (next ? next[0].length : 0)] || "";
      const probe = grammar.caseInsensitive ? value.toLowerCase() : value;
      const inList = (list) =>
        list.some((entry) =>
          grammar.caseInsensitive ? entry.toLowerCase() === probe : entry === value,
        );
      let cls = "";
      if (inList(keywords)) cls = "tok-key";
      else if (inList(types)) cls = "tok-type";
      else if (grammar.keys && nextChar === ":") cls = "tok-prop";
      else if (nextChar === "(") cls = "tok-fn";
      else if (grammar.capitalized && /^[A-Z]/.test(value)) cls = "tok-type";
      if (cls) span(cls, value);
      else plain += value;
      index = after;
      continue;
    }
    plain += ch;
    index += 1;
  }
  flush();
  return out.join("");
}

/* Markup: comments, the tag name, attribute names and quoted values. Everything
   else is text, escaped. */
function tokenizeMarkup(text) {
  const out = [];
  let plain = "";
  const flush = () => {
    if (plain) {
      out.push(escapeCode(plain));
      plain = "";
    }
  };
  let index = 0;
  while (index < text.length) {
    if (text.startsWith("<!--", index)) {
      const closeAt = text.indexOf("-->", index);
      const stop = closeAt === -1 ? text.length : closeAt + 3;
      flush();
      out.push(`<span class="tok-com">${escapeCode(text.slice(index, stop))}</span>`);
      index = stop;
      continue;
    }
    if (text[index] === "<") {
      const closeAt = text.indexOf(">", index);
      if (closeAt !== -1) {
        flush();
        out.push(colorTag(text.slice(index, closeAt + 1)));
        index = closeAt + 1;
        continue;
      }
    }
    plain += text[index];
    index += 1;
  }
  flush();
  return out.join("");
}

/* One tag, scanned rather than rewritten: a regex over the freshly inserted
   spans would recolour the `class="tok-tag"` it had just written. */
function colorTag(raw) {
  const limit = raw.endsWith(">") ? raw.length - 1 : raw.length;
  let index = 1;
  let html = "&lt;";
  if (raw[1] === "/") {
    html += "/";
    index = 2;
  }
  const name = /^[A-Za-z][\w:.-]*/.exec(raw.slice(index, limit));
  if (name) {
    html += `<span class="tok-tag">${escapeCode(name[0])}</span>`;
    index += name[0].length;
  }
  let plain = "";
  const flush = () => {
    if (plain) {
      html += escapeCode(plain);
      plain = "";
    }
  };
  while (index < limit) {
    const ch = raw[index];
    if (/\s/.test(ch)) {
      plain += ch;
      index += 1;
      continue;
    }
    if (ch === '"' || ch === "'") {
      const closeAt = raw.indexOf(ch, index + 1);
      const stop = closeAt === -1 || closeAt > limit ? limit : closeAt + 1;
      flush();
      html += `<span class="tok-str">${escapeCode(raw.slice(index, stop))}</span>`;
      index = stop;
      continue;
    }
    if (ch === "=") {
      flush();
      html += "=";
      index += 1;
      continue;
    }
    const word = /^[A-Za-z_:][-A-Za-z0-9_:.]*/.exec(raw.slice(index, limit));
    if (word) {
      const after = index + word[0].length;
      const attribute = /^\s*=/.test(raw.slice(after, limit));
      flush();
      html += attribute
        ? `<span class="tok-attr">${escapeCode(word[0])}</span>`
        : escapeCode(word[0]);
      index = after;
      continue;
    }
    plain += ch;
    index += 1;
  }
  flush();
  return raw.endsWith(">") ? `${html}&gt;` : html;
}

/* Markdown: fences, headings, inline code, links and list markers. Emphasis is
   left plain on purpose. */
function tokenizeMarkdown(text) {
  let fence = "";
  return text
    .split("\n")
    .map((line) => {
      const fenceMatch = /^\s*(```|~~~)/.exec(line);
      if (fenceMatch) {
        if (!fence) {
          fence = fenceMatch[1];
          return `<span class="tok-str">${escapeCode(line)}</span>`;
        }
        if (fenceMatch[1] === fence) {
          fence = "";
          return `<span class="tok-str">${escapeCode(line)}</span>`;
        }
      }
      if (fence) return `<span class="tok-str">${escapeCode(line)}</span>`;
      if (/^\s{0,3}#{1,6}\s/.test(line)) {
        return `<span class="tok-key">${escapeCode(line)}</span>`;
      }
      let html = escapeCode(line);
      html = html.replace(/(`[^`]*`)/g, '<span class="tok-str">$1</span>');
      html = html.replace(
        /(\[[^\]]*\])(\([^)]*\))/g,
        '<span class="tok-fn">$1</span><span class="tok-str">$2</span>',
      );
      html = html.replace(
        /^(\s*)([-*+]|\d+\.)(\s)/,
        (match, pad, marker, space) =>
          `${pad}<span class="tok-punct">${marker}</span>${space}`,
      );
      return html;
    })
    .join("\n");
}

/** Escaped, coloured HTML for `text`; plain escaped text when the language is
 * unknown or the buffer is over the cap. */
function highlightCode(text, language) {
  const source = String(text === null || text === undefined ? "" : text);
  if (!source) return "";
  const grammarKey = highlightGrammar(language);
  if (!grammarKey || source.length > HIGHLIGHT_LIMIT) return escapeCode(source);
  let html;
  if (grammarKey === "markdown") html = tokenizeMarkdown(source);
  else if (grammarKey === "html") html = tokenizeMarkup(source);
  else html = tokenizeCode(source, GRAMMARS[grammarKey]);
  // A trailing newline is swallowed by <pre>; one extra character keeps the
  // overlay as tall as the textarea's buffer.
  return source.endsWith("\n") ? `${html} ` : html;
}

if (typeof window !== "undefined") {
  window.SarthiHighlight = { highlightCode, escapeCode, highlightSupported };
}
