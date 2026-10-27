// Build script for the Free AI Studio VS Code extension.
//
// Two bundles:
//   * dist/extension.js  — the extension host bundle (CommonJS, `vscode` external)
//   * dist/lib.mjs       — the same pure logic as an ES module so `node --test`
//                          can exercise it without a VS Code host
//
// The desktop UI is copied to media/ui so the built-in server (and the packaged
// .vsix) ships exactly the same interface as the desktop app.

import { build } from "esbuild";
import { cp, mkdir, readdir, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const withTests = process.argv.includes("--with-tests");

async function copyUi() {
  const source = join(here, "..", "app", "static");
  if (!existsSync(source)) {
    throw new Error(`Desktop UI not found at ${source}`);
  }
  const target = join(here, "media", "ui");
  await rm(target, { recursive: true, force: true });
  await mkdir(target, { recursive: true });
  for (const name of await readdir(source, { withFileTypes: true })) {
    if (name.name.endsWith(".map")) continue;
    await cp(join(source, name.name), join(target, name.name), {
      recursive: true,
    });
  }
}

await rm(join(here, "dist"), { recursive: true, force: true });
await mkdir(join(here, "dist"), { recursive: true });
await copyUi();

await build({
  entryPoints: [join(here, "src", "extension.ts")],
  outfile: join(here, "dist", "extension.js"),
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["vscode"],
  sourcemap: true,
  logLevel: "info",
});

if (withTests) {
  await build({
    entryPoints: [join(here, "src", "lib.ts")],
    outfile: join(here, "dist", "lib.mjs"),
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node18",
    logLevel: "info",
  });
}
