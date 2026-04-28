#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, copyFileSync } from "node:fs";
import { dirname, basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);

const args = process.argv.slice(2);
const platformArgIndex = args.findIndex((arg) => arg === "--platform");
const modeArgIndex = args.findIndex((arg) => arg === "--mode");
const profileArgIndex = args.findIndex((arg) => arg === "--profile");

const platform = platformArgIndex >= 0 ? args[platformArgIndex + 1] : process.platform;
const mode = modeArgIndex >= 0 ? args[modeArgIndex + 1] : "dev";
const profile = profileArgIndex >= 0 ? args[profileArgIndex + 1] : "debug";

if (!["linux", "win32"].includes(platform)) {
  console.log(`[INFO] apply_runtime_libs skipped for platform: ${platform}`);
  process.exit(0);
}

if (!["dev", "bundle"].includes(mode)) {
  console.error(`[ERROR] Unsupported mode: ${mode}`);
  process.exit(1);
}

if (!["debug", "release"].includes(profile)) {
  console.error(`[ERROR] Unsupported profile: ${profile}`);
  process.exit(1);
}

const manifestPath = resolve(projectRoot, "src-tauri", "libs", `runtime-libs.${platform}.json`);
if (!existsSync(manifestPath)) {
  console.error(`[ERROR] Runtime manifest not found: ${manifestPath}`);
  console.error(
    `[ERROR] Generate it with: node ./scripts/sync_runtime_libs.mjs --platform ${platform}`
  );
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const entries = Array.isArray(manifest.entries) ? manifest.entries : [];
if (entries.length === 0) {
  console.error(`[ERROR] Runtime manifest has no entries: ${manifestPath}`);
  process.exit(1);
}

const destinationDir =
  mode === "dev"
    ? resolve(projectRoot, "src-tauri", "target", "debug")
    : resolve(projectRoot, "src-tauri", "target", profile);

mkdirSync(destinationDir, { recursive: true });

let copied = 0;
for (const relativePath of entries) {
  const source = resolve(projectRoot, "src-tauri", relativePath);
  if (!existsSync(source)) {
    console.error(`[ERROR] Missing runtime lib referenced in manifest: ${relativePath}`);
    process.exit(1);
  }

  const target = resolve(destinationDir, basename(relativePath));
  copyFileSync(source, target);
  copied += 1;
}

console.log(
  `[INFO] Applied runtime libs (${platform}, ${mode}, ${profile}): ${copied} -> ${destinationDir}`
);
