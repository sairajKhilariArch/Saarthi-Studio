"use strict";

/* Importing a skill from Git: the form, the request the page makes, and the
 * refresh afterwards. The clone itself is Rust's and is tested there against a
 * real local repository.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const read = (name) => fs.readFileSync(path.join(__dirname, "..", name), "utf8");

test("the skills pane has a Git import form, wired to the import route", () => {
  const html = read("index.html");
  assert.match(html, /<details id="skillImport" class="skill-install">/);
  assert.match(html, /id="skillImportSource"/);
  assert.match(html, /id="skillImportRef"/);
  assert.match(html, /id="skillImportBtn"/);
  // A local folder without .git is copied, not cloned.
  assert.match(html, /a local folder without <code>\.git<\/code> is copied/);
  assert.match(html, /needs a SKILL\.md at its root/);

  const app = read("app.js");
  assert.match(app, /function wireSkillImport\(\)/);
  assert.match(app, /wireSkillImport\(\);/);
  assert.match(app, /async function importSkill\(\)/);
  assert.match(app, /api\("\/api\/skills\/import", \{\n    method: "POST",\n    confirmApproval: true,/);
  // The optional ref goes as its own field, and the list refreshes afterwards.
  assert.match(app, /JSON\.stringify\(\{ source, git_ref: \$\("skillImportRef"\)\.value\.trim\(\) \}\)/);
  assert.match(app, /await loadSkills\(\);\n  await openSkillDetail\(result\.data\.id\);/);
});

test("an imported skill offers Update from Git, which clones its recorded origin", () => {
  const app = read("app.js");
  assert.match(app, /async function refreshSkill\(id, origin\)/);
  assert.match(app, /if \(skill\.origin\) \{/);
  assert.match(app, /api\(`\/api\/skills\/\$\{encodeURIComponent\(id\)\}\/refresh`, \{\n    method: "POST",\n    confirmApproval: true,/);
  // Replacing instructions is confirmed, and the old copy is named as kept.
  assert.match(app, /Replace this skill's instructions with the current version of \$\{origin\}/);
  assert.match(app, /kept under \.removed/);
});

test("the server records and refreshes a Git origin", () => {
  const rust = read("../src/skills.rs");
  assert.match(rust, /\.route\("\/api\/skills\/:id\/refresh", post\(refresh_skill\)\)/);
  assert.match(rust, /fn refresh_clone\(/);
  assert.match(rust, /The repository now names this skill/);
  // The origin comes from the registry, not from the request body.
  assert.match(rust, /let origin = record\.origin\.clone\(\);/);
  assert.match(rust, /\"origin\": record\.as_ref\(\)\.map\(\|entry\| entry\.origin\.clone\(\)\)/);
  assert.match(rust, /entry\.origin = origin\.to_string\(\);/);
});

test("the server route is registered and the clone is bounded", () => {
  const rust = read("../src/skills.rs");
  assert.match(rust, /\.route\("\/api\/skills\/import", post\(import_skill\)\)/);
  assert.match(rust, /fn import_clone\(root: &FsPath, source: &str, git_ref: Option<&str>\)/);
  assert.match(rust, /Use an https:\/\/ URL; plain http is refused\./);
  assert.match(rust, /const IMPORT_MAX_BYTES: u64 = 2 \* 1024 \* 1024;/);
  assert.match(rust, /if kind\.is_symlink\(\)/);
  // The permission decision is a git one, and it names the write.
  assert.match(rust, /clone a skill from a git repository into the skills folder/);
});
