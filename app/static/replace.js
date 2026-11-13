"use strict";

/* Literal find and replace for the editor. Pure text in, text out: the find
 * bar's navigation and the buffer write live in ide.js, and the matching rule
 * here is deliberately the one findMatches uses — a lowercase comparison of
 * non-overlapping occurrences, so the count in the bar and the number Replace
 * all reports always agree.
 *
 * The replacement is inserted as text, never as a pattern: `$&` and `$1` stay
 * exactly as typed. That is why this is a scan rather than a RegExp built from
 * the needle.
 */

/**
 * Replace literal occurrences of `needle` with `replacement`.
 * `limit` caps how many occurrences are replaced (null or omitted = all).
 * Returns the new text, how many were replaced, and the offset just past the
 * last replacement (0 when nothing changed).
 */
function replaceLiteral(text, needle, replacement, limit) {
  const source = String(text == null ? "" : text);
  const find = String(needle == null ? "" : needle);
  const insert = String(replacement == null ? "" : replacement);
  if (!find) return { text: source, count: 0, end: 0 };
  const hay = source.toLowerCase();
  const target = find.toLowerCase();
  const cap = typeof limit === "number" && limit >= 0 ? limit : null;
  let out = "";
  let cursor = 0;
  let count = 0;
  let end = 0;
  let index = hay.indexOf(target);
  while (index !== -1 && (cap === null || count < cap)) {
    out += source.slice(cursor, index) + insert;
    cursor = index + find.length;
    end = cursor - find.length + insert.length;
    count += 1;
    index = hay.indexOf(target, cursor);
  }
  if (!count) return { text: source, count: 0, end: 0 };
  return { text: out + source.slice(cursor), count, end };
}
