#!/usr/bin/env node
import { existsSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { delimiter, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);
const syncScript = resolve(scriptDir, "sync_runtime_libs.mjs");
const applyScript = resolve(scriptDir, "apply_runtime_libs.mjs");
const tauriRuntimeMacConfigPath = resolve(projectRoot, "src-tauri", "tauri.runtime.macos.json");
const runtimeLibsDir = resolve(projectRoot, "src-tauri", "libs");
const mpvRuntimeLibsDir = resolve(runtimeLibsDir, "mpv");
const devFrameworksDir = resolve(projectRoot, "src-tauri", "target", "Frameworks");
const devVulkanIcdPath = resolve(
  projectRoot,
  "src-tauri",
  "target",
  "Frameworks",
  "vulkan",
  "icd.d",
  "MoltenVK_icd.json"
);
const libsVulkanIcdPath = resolve(projectRoot, "src-tauri", "libs", "mpv", "MoltenVK_icd.json");
const tauriArgs = process.argv.slice(2);
const tauriSubcommand = tauriArgs.find((arg) => arg === "dev" || arg === "build") ?? "";
const isDevOrBuild = tauriSubcommand === "dev" || tauriSubcommand === "build";
const isDev = tauriSubcommand === "dev";
const skipApplyRuntimeLibs = process.env.SOIA_SKIP_APPLY_RUNTIME_LIBS === "1";

function runNodeScriptOrExit(commandArgs) {
  const result = spawnSync(process.execPath, commandArgs, {
    cwd: projectRoot,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function hasExecutableOnPath(command) {
  const result = spawnSync(command, ["--version"], {
    cwd: projectRoot,
    stdio: "ignore",
    env: process.env,
  });
  return result.status === 0;
}

function hasUserConfigArg(args) {
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "-c" || arg === "--config") {
      return true;
    }
    if (arg.startsWith("--config=")) {
      return true;
    }
  }
  return false;
}

if (isDevOrBuild) {
  if (!hasExecutableOnPath("cargo")) {
    console.error("[ERROR] Missing required command: cargo");
    console.error("[ERROR] Install Rust toolchain and ensure cargo is in PATH.");
    console.error("[ERROR] Recommended: https://rustup.rs/");
    process.exit(1);
  }

  if (process.platform === "darwin") {
    runNodeScriptOrExit([syncScript, "--platform", "darwin", "--check"]);
  } else if (process.platform === "linux" || process.platform === "win32") {
    runNodeScriptOrExit([syncScript, "--platform", process.platform, "--check"]);
    if (!skipApplyRuntimeLibs) {
      if (tauriSubcommand === "dev") {
        runNodeScriptOrExit([
          applyScript,
          "--platform",
          process.platform,
          "--mode",
          "dev",
          "--profile",
          "debug",
        ]);
      } else if (tauriSubcommand === "build") {
        const profile = tauriArgs.includes("--debug") ? "debug" : "release";
        runNodeScriptOrExit([
          applyScript,
          "--platform",
          process.platform,
          "--mode",
          "bundle",
          "--profile",
          profile,
        ]);
      }
    } else {
      console.log("[INFO] Skip apply_runtime_libs because SOIA_SKIP_APPLY_RUNTIME_LIBS=1");
    }
  }
}

if (isDevOrBuild && process.platform === "darwin" && !hasUserConfigArg(tauriArgs)) {
  if (!existsSync(tauriRuntimeMacConfigPath)) {
    console.error(`[ERROR] Missing macOS runtime config: ${tauriRuntimeMacConfigPath}`);
    console.error("[ERROR] Run: pnpm setup:libs");
    process.exit(1);
  }
  tauriArgs.push("--config", tauriRuntimeMacConfigPath);
}

const tauriCmd = "tauri";
const childEnv = { ...process.env };

function resolveEnvKey(envKey) {
  if (process.platform !== "win32") {
    return envKey;
  }
  const found = Object.keys(childEnv).find((key) => key.toLowerCase() === envKey.toLowerCase());
  return found ?? envKey;
}

function prependEnvPath(envKey, extraPaths) {
  const resolvedKey = resolveEnvKey(envKey);
  const existing = childEnv[resolvedKey] ?? "";
  const merged = [...extraPaths, ...existing.split(delimiter).filter(Boolean)];
  const deduped = [...new Set(merged)];
  childEnv[resolvedKey] = deduped.join(delimiter);

  if (process.platform === "win32" && resolvedKey.toLowerCase() === "path") {
    for (const key of Object.keys(childEnv)) {
      if (key !== resolvedKey && key.toLowerCase() === "path") {
        delete childEnv[key];
      }
    }
  }
}

if (isDev && existsSync(runtimeLibsDir)) {
  if (process.platform === "darwin") {
    const extra = [runtimeLibsDir];
    if (existsSync(mpvRuntimeLibsDir)) {
      extra.push(mpvRuntimeLibsDir);
    }
    if (existsSync(devFrameworksDir)) {
      extra.push(devFrameworksDir);
    }
    prependEnvPath("DYLD_FALLBACK_LIBRARY_PATH", extra);

    const vulkanIcdCandidates = [];
    if (existsSync(devVulkanIcdPath)) {
      vulkanIcdCandidates.push(devVulkanIcdPath);
    }
    if (existsSync(libsVulkanIcdPath)) {
      vulkanIcdCandidates.push(libsVulkanIcdPath);
    }

    if (vulkanIcdCandidates.length > 0) {
      const dedupedIcdList = [...new Set(vulkanIcdCandidates)].join(delimiter);
      const vkDriverFiles = childEnv.VK_DRIVER_FILES;
      const vkIcdFilenames = childEnv.VK_ICD_FILENAMES;
      childEnv.VK_DRIVER_FILES = vkDriverFiles
        ? `${dedupedIcdList}${delimiter}${vkDriverFiles}`
        : dedupedIcdList;
      childEnv.VK_ICD_FILENAMES = vkIcdFilenames
        ? `${dedupedIcdList}${delimiter}${vkIcdFilenames}`
        : dedupedIcdList;
    }
  } else if (process.platform === "linux") {
    const extra = [runtimeLibsDir];
    if (existsSync(mpvRuntimeLibsDir)) {
      extra.push(mpvRuntimeLibsDir);
    }
    prependEnvPath("LD_LIBRARY_PATH", extra);
  } else if (process.platform === "win32") {
    const extra = [runtimeLibsDir];
    if (existsSync(mpvRuntimeLibsDir)) {
      extra.push(mpvRuntimeLibsDir);
    }
    prependEnvPath("PATH", extra);
  }
}

const child =
  process.platform === "win32"
    ? spawn("cmd.exe", ["/d", "/s", "/c", tauriCmd, ...tauriArgs], {
        env: childEnv,
        stdio: "inherit",
      })
    : spawn(tauriCmd, tauriArgs, {
        env: childEnv,
        stdio: "inherit",
      });

child.on("error", (error) => {
  console.error(`[ERROR] Failed to run ${tauriCmd}: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
