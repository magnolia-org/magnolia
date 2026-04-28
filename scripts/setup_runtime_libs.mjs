#!/usr/bin/env node
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);
const shellScript = resolve(scriptDir, "setup_runtime_libs_macos.sh");
const linuxShellScript = resolve(scriptDir, "setup_runtime_libs_linux.sh");
const windowsScript = resolve(scriptDir, "setup_runtime_libs_windows.mjs");
const rawArgs = process.argv.slice(2);
const passthroughArgs = [];

let requestedPlatform;
for (let i = 0; i < rawArgs.length; i += 1) {
  const arg = rawArgs[i];
  if (arg === "--platform") {
    const next = rawArgs[i + 1];
    if (!next) {
      console.error("[ERROR] Missing value for --platform");
      process.exit(1);
    }
    requestedPlatform = next;
    i += 1;
    continue;
  }
  passthroughArgs.push(arg);
}

function normalizePlatform(input) {
  if (!input) return process.platform;
  const value = input.toLowerCase();
  if (value === "darwin" || value === "mac" || value === "macos" || value === "osx") {
    return "darwin";
  }
  if (value === "win" || value === "windows" || value === "win32") {
    return "win32";
  }
  if (value === "linux") {
    return "linux";
  }
  if (value === "android") {
    return "android";
  }
  return value;
}

const platform = normalizePlatform(requestedPlatform);
const isExplicitPlatformRequest = typeof requestedPlatform === "string" && requestedPlatform.length > 0;

if (!["darwin", "linux", "win32", "android"].includes(platform)) {
  console.error(`[ERROR] Unsupported platform: ${platform}`);
  console.error("[ERROR] Supported values: darwin, linux, win32, android");
  process.exit(1);
}

if (platform === "darwin") {
  if (process.platform !== "darwin") {
    console.error("[ERROR] macOS runtime setup requires a macOS host.");
    process.exit(1);
  }

  if (!existsSync(shellScript)) {
    console.error(`[ERROR] Missing script: ${shellScript}`);
    process.exit(1);
  }

  const result = spawnSync("bash", [shellScript, ...passthroughArgs], {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
  });

  process.exit(result.status ?? 1);
}

if (platform === "linux") {
  if (process.platform !== "linux") {
    console.error("[ERROR] Linux runtime setup requires a Linux host.");
    process.exit(1);
  }

  if (!existsSync(linuxShellScript)) {
    console.error(`[ERROR] Missing script: ${linuxShellScript}`);
    process.exit(1);
  }

  const result = spawnSync("bash", [linuxShellScript, ...passthroughArgs], {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
  });

  process.exit(result.status ?? 1);
}

if (platform === "win32") {
  if (process.platform !== "win32") {
    console.error("[ERROR] Windows runtime setup requires a Windows host.");
    process.exit(1);
  }

  if (!existsSync(windowsScript)) {
    console.error(`[ERROR] Missing script: ${windowsScript}`);
    process.exit(1);
  }

  const result = spawnSync(process.execPath, [windowsScript, ...passthroughArgs], {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
  });

  process.exit(result.status ?? 1);
}

if (!isExplicitPlatformRequest) {
  console.log("[INFO] setup:libs skipped on android (not implemented yet).");
  process.exit(0);
}

console.error("[ERROR] TODO: add scripts/setup_runtime_libs_android.mjs and dispatch here.");
process.exit(1);
