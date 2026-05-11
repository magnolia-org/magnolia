#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
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

const manifestPath = resolve(projectRoot, "src-tauri", "libs", "runtime-libs.linux.json");
const bundleRoot = resolve(projectRoot, "src-tauri", "target", profile, "bundle");

if (!existsSync(manifestPath)) {
  console.error(`[ERROR] Runtime manifest not found: ${manifestPath}`);
  console.error("[ERROR] Generate it on Linux with: npm run sync:runtime:linux");
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

function runOrThrow(command, commandArgs, cwd) {
  const result = spawnSync(command, commandArgs, { cwd, stdio: "pipe", encoding: "utf8" });
  if (result.status === 0) {
    return;
  }

  const output = [result.stdout ?? "", result.stderr ?? ""].join("\n").trim();
  const suffix = output ? `\n${output}` : "";
  throw new Error(`Command failed: ${command} ${commandArgs.join(" ")}${suffix}`);
}

function walkTree(rootDir, onDirectory, onFile) {
  const queue = [rootDir];
  while (queue.length > 0) {
    const current = queue.pop();
    if (!current) continue;

    const children = readdirSync(current, { withFileTypes: true });
    for (const child of children) {
      const fullPath = resolve(current, child.name);
      if (child.isDirectory()) {
        if (onDirectory) onDirectory(fullPath);
        queue.push(fullPath);
        continue;
      }
      if (child.isFile() && onFile) {
        onFile(fullPath);
      }
    }
  }
}

function shouldIncludeBundleDir(bundleDir) {
  const rel = relative(bundleRoot, bundleDir).split(sep).join("/");
  if (!rel || rel.startsWith("..")) return false;

  if (rel.endsWith("/usr/bin") || rel === "usr/bin") return true;
  if (rel.endsWith("/usr/lib") || rel === "usr/lib") return true;
  if (rel.includes("/usr/lib/")) return true;
  if (rel.includes(".AppDir/usr/bin")) return true;
  if (rel.includes(".AppDir/usr/lib")) return true;
  return false;
}

function shouldIncludeDebDataDir(dataRoot, dataDir) {
  const rel = relative(dataRoot, dataDir).split(sep).join("/");
  if (!rel || rel.startsWith("..")) return false;

  if (rel === "usr/lib") return true;
  if (rel.startsWith("usr/lib/")) return true;
  return false;
}

function isExcludedRuntimeSharedObject(name) {
  return [
    /^libEGL\.so(\..+)?$/,
    /^libGL\.so(\..+)?$/,
    /^libGLX\.so(\..+)?$/,
    /^libGLdispatch\.so(\..+)?$/,
    /^libGLES.*\.so(\..+)?$/,
    /^libgbm\.so(\..+)?$/,
    /^libdrm\.so(\..+)?$/,
    /^libwayland.*\.so(\..+)?$/,
  ].some((pattern) => pattern.test(name));
}

function removeExcludedRuntimeLibsFromDir(dirPath) {
  if (!existsSync(dirPath)) {
    return 0;
  }

  let removed = 0;
  const entries = readdirSync(dirPath, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile() && !entry.isSymbolicLink()) continue;
    if (!isExcludedRuntimeSharedObject(entry.name)) continue;
    rmSync(resolve(dirPath, entry.name), { force: true });
    removed += 1;
  }

  return removed;
}

function removeRuntimeLibsFromUsrBin(dataRoot) {
  const usrBinDir = resolve(dataRoot, "usr", "bin");
  if (!existsSync(usrBinDir)) {
    return 0;
  }

  let removed = 0;
  const entries = readdirSync(usrBinDir, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile() && !entry.isSymbolicLink()) continue;
    if (!/\.so(\..+)?$/.test(entry.name)) continue;
    rmSync(resolve(usrBinDir, entry.name), { force: true });
    removed += 1;
  }
  return removed;
}

function collectBundleTargetDirs() {
  const targetDirs = new Set();
  walkTree(
    bundleRoot,
    (fullPath) => {
      if (shouldIncludeBundleDir(fullPath)) {
        targetDirs.add(fullPath);
      }
    },
    null
  );
  return targetDirs;
}

function collectDebFiles() {
  const debFiles = [];
  walkTree(
    bundleRoot,
    null,
    (fullPath) => {
      if (fullPath.endsWith(".deb")) {
        debFiles.push(fullPath);
      }
    }
  );
  return debFiles.sort((a, b) => a.localeCompare(b));
}

function copyRuntimeLibsToDirs(targetDirs, runtimeLibSources) {
  let copied = 0;
  let removedExcluded = 0;
  for (const targetDir of targetDirs) {
    mkdirSync(targetDir, { recursive: true });
    removedExcluded += removeExcludedRuntimeLibsFromDir(targetDir);
    for (const source of runtimeLibSources) {
      const target = resolve(targetDir, basename(source));
      copyFileSync(source, target);
      copied += 1;
    }
  }
  return { copied, removedExcluded };
}

function detectDataArchiveKind(dataArchiveName) {
  if (dataArchiveName.endsWith(".tar.xz")) return "xz";
  if (dataArchiveName.endsWith(".tar.gz")) return "gz";
  if (dataArchiveName.endsWith(".tar.bz2")) return "bz2";
  if (dataArchiveName.endsWith(".tar.zst")) return "zst";
  if (dataArchiveName.endsWith(".tar")) return "plain";
  throw new Error(`Unsupported data archive format in deb: ${dataArchiveName}`);
}

function rebuildDataArchive(dataRoot, dataArchivePath, kind) {
  const ownershipArgs = ["--numeric-owner", "--owner=0", "--group=0"];
  const argsByKind = {
    xz: ["-cJf", dataArchivePath, ...ownershipArgs, "-C", dataRoot, "."],
    gz: ["-czf", dataArchivePath, ...ownershipArgs, "-C", dataRoot, "."],
    bz2: ["-cjf", dataArchivePath, ...ownershipArgs, "-C", dataRoot, "."],
    zst: ["-c", "--zstd", "-f", dataArchivePath, ...ownershipArgs, "-C", dataRoot, "."],
    plain: ["-cf", dataArchivePath, ...ownershipArgs, "-C", dataRoot, "."],
  };

  const tarArgs = argsByKind[kind];
  if (!tarArgs) {
    throw new Error(`Unsupported data archive kind: ${kind}`);
  }
  runOrThrow("tar", tarArgs, undefined);
}

function patchDebPackage(debPath, runtimeLibSources) {
  const tempRoot = mkdtempSync(resolve(tmpdir(), "magnolia-linux-bundle-"));
  let copied = 0;
  let targetDirCount = 0;
  let removedExcluded = 0;

  try {
    runOrThrow("ar", ["x", debPath], tempRoot);

    const extracted = readdirSync(tempRoot, { withFileTypes: true })
      .filter((entry) => entry.isFile())
      .map((entry) => entry.name);

    const dataArchiveName = extracted.find((name) => name.startsWith("data.tar"));
    const controlArchiveName = extracted.find((name) => name.startsWith("control.tar"));
    if (!dataArchiveName || !controlArchiveName || !extracted.includes("debian-binary")) {
      throw new Error(`Invalid deb layout while patching: ${debPath}`);
    }

    const dataArchivePath = resolve(tempRoot, dataArchiveName);
    const dataRoot = resolve(tempRoot, "data-root");
    mkdirSync(dataRoot, { recursive: true });

    runOrThrow("tar", ["-xf", dataArchivePath, "-C", dataRoot], undefined);
    const removedFromUsrBin = removeRuntimeLibsFromUsrBin(dataRoot);

    const targetDirs = new Set();
    walkTree(
      dataRoot,
      (fullPath) => {
        if (shouldIncludeDebDataDir(dataRoot, fullPath)) {
          targetDirs.add(fullPath);
        }
      },
      null
    );

    if (targetDirs.size === 0) {
      // Keep a safe fallback so libs are still packaged when deb layout changes.
      const fallback = resolve(dataRoot, "usr", "lib");
      targetDirs.add(fallback);
    }

    const copyResult = copyRuntimeLibsToDirs(targetDirs, runtimeLibSources);
    copied += copyResult.copied;
    removedExcluded += copyResult.removedExcluded;
    targetDirCount = targetDirs.size;

    const archiveKind = detectDataArchiveKind(dataArchiveName);
    rebuildDataArchive(dataRoot, dataArchivePath, archiveKind);

    const rebuiltDebPath = resolve(tempRoot, "rebuilt.deb");
    runOrThrow(
      "ar",
      ["rcs", rebuiltDebPath, "debian-binary", controlArchiveName, dataArchiveName],
      tempRoot
    );
    copyFileSync(rebuiltDebPath, debPath);

    return { copied, targetDirs: targetDirCount, removedFromUsrBin, removedExcluded };
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
}

function main() {
  const expandedTargetDirs = collectBundleTargetDirs();
  const debFiles = collectDebFiles();

  if (expandedTargetDirs.size === 0 && debFiles.length === 0) {
    const message = `[WARN] No Linux bundle targets found under: ${bundleRoot}`;
    if (strict) {
      console.error(message.replace("[WARN]", "[ERROR]"));
      process.exit(1);
    }
    console.warn(message);
    process.exit(0);
  }

  let copied = 0;
  let removedExcluded = 0;

  if (expandedTargetDirs.size > 0) {
    const copyResult = copyRuntimeLibsToDirs(expandedTargetDirs, sources);
    copied += copyResult.copied;
    removedExcluded += copyResult.removedExcluded;
  }

  let debPatchedCount = 0;
  let debTargetDirs = 0;
  let removedFromUsrBin = 0;
  for (const debPath of debFiles) {
    const result = patchDebPackage(debPath, sources);
    copied += result.copied;
    debPatchedCount += 1;
    debTargetDirs += result.targetDirs;
    removedFromUsrBin += result.removedFromUsrBin;
    removedExcluded += result.removedExcluded;
  }

  const expandedMessage =
    expandedTargetDirs.size > 0 ? `expanded-dirs=${expandedTargetDirs.size}` : "expanded-dirs=0";
  const debMessage =
    debPatchedCount > 0
      ? `deb-packages=${debPatchedCount},deb-target-dirs=${debTargetDirs},removed-usr-bin-libs=${removedFromUsrBin}`
      : "deb-packages=0";

  console.log(
    `[INFO] Linux bundle runtime libs applied: ${sources.length} libs, ${expandedMessage}, ${debMessage}, removed-excluded=${removedExcluded}, copies=${copied}`
  );
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`[ERROR] ${message}`);
  process.exit(1);
}
