const s = require('fs').readFileSync(process.argv[2] || 'app.static/app.js', 'utf8');
let depth = 0, line = 1, inS = null, esc = false;
const stack = [];
for (let i = 0; i < s.length; i++) {
  const c = s[i], n = s[i + 1];
  if (c === '\n') line++;
  if (inS) {
    if (esc) { esc = false; continue; }
    if (c === '\\') { esc = true; continue; }
    if (c === inS) inS = null;
    continue;
  }
  if (c === '"' || c === "'" || c === '`') { inS = c; continue; }
  if (c === '/' && n === '/') { while (i < s.length && s[i] !== '\n') i++; continue; }
  if (c === '/' && n === '*') {
    i += 2;
    while (i < s.length && !(s[i] === '*' && s[i + 1] === '/')) { if (s[i] === '\n') line++; i++; }
    i++; continue;
  }
  if (c === '{' || c === '(' || c === '[') { depth++; stack.push([c, line]); }
  if (c === '}' || c === ')' || c === ']') {
    depth--;
    const match = { '}': '{', ')': '(', ']': '[' }[c];
    if (stack.length && stack[stack.length - 1][0] === match) stack.pop();
    else if (depth < 0) { console.log('extra', c, 'at line', line); depth = 0; }
  }
}
console.log('final depth', depth);
console.log('unclosed (last 8):', stack.slice(-8));
