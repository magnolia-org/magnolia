#!/usr/bin/env node
import { copyFileSync, existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, basename, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);

const args = process.argv.slice(2);
const profileArgIndex = args.findIndex((arg) => arg === "--profile");
const strict = args.includes("--strict");
const profile = profileArgIndex >= 0 ? args[profileArgIndex + 1] : "release";

if (!["debug", "release"].includes(profile)) {
  console.error(`[ERROR] Unsupported profile: ${profile}`);
  process.exit(1);
}

const manifestPath = resolve(projectRoot, "src-tauri", "libs", "runtime-libs.win32.json");
const bundleRoot = resolve(projectRoot, "src-tauri", "target", profile, "bundle");

if (!existsSync(manifestPath)) {
  console.error(`[ERROR] Runtime manifest not found: ${manifestPath}`);
  console.error("[ERROR] Generate it on Windows with: npm run sync:runtime:win");
  process.exit(1);
}

if (!existsSync(bundleRoot)) {
  console.error(`[ERROR] Bundle output directory not found: ${bundleRoot}`);
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const entries = Array.isArray(manifest.entries) ? manifest.entries : [];
if (entries.length === 0) {
  console.error(`[ERROR] Runtime manifest has no entries: ${manifestPath}`);
  process.exit(1);
}

const sources = entries.map((relativeEntry) => {
  const source = resolve(projectRoot, "src-tauri", relativeEntry);
  if (!existsSync(source)) {
    console.error(`[ERROR] Missing runtime lib referenced in manifest: ${relativeEntry}`);
    process.exit(1);
  }
  return source;
});

const targetDirs = new Set();

function walk(dir) {
  const entries = readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      walk(fullPath);
      continue;
    }

    if (!entry.name.toLowerCase().endsWith(".exe")) {
      continue;
    }

    const rel = relative(bundleRoot, fullPath).split(sep).join("/");
    if (!rel || rel.startsWith("..")) {
      continue;
    }
    const relLower = rel.toLowerCase();
    const nameLower = entry.name.toLowerCase();

    // Ignore installer stubs if possible; prefer app payload dirs.
    if (relLower.startsWith("nsis/") && (nameLower.includes("setup") || nameLower.includes("installer"))) {
      continue;
    }

    targetDirs.add(dirname(fullPath));
  }
}

walk(bundleRoot);

if (targetDirs.size === 0) {
  const message = `[WARN] No Windows .exe payload directories found under: ${bundleRoot}`;
  if (strict) {
    console.error(message.replace("[WARN]", "[ERROR]"));
    process.exit(1);
  }
  console.warn(message);
  process.exit(0);
}

let copied = 0;
for (const targetDir of targetDirs) {
  for (const source of sources) {
    const target = resolve(targetDir, basename(source));
    copyFileSync(source, target);
    copied += 1;
  }
}

console.log(
  `[INFO] Windows bundle runtime libs applied: ${sources.length} libs x ${targetDirs.size} dirs = ${copied} copies`
);
