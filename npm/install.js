#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const os = require("os");
const { execSync } = require("child_process");

const VERSION = require("./package.json").version;

const PLATFORM_PACKAGES = {
  "darwin-arm64": "@intuit/infigraph-darwin-arm64",
  "darwin-x64": "@intuit/infigraph-darwin-x64",
  "linux-x64": "@intuit/infigraph-linux-x64",
  "win32-x64": "@intuit/infigraph-win32-x64",
};

const PLATFORM_TARGETS = {
  "darwin-arm64": { target: "aarch64-apple-darwin", ext: "tar.gz" },
  "darwin-x64": { target: "x86_64-apple-darwin", ext: "tar.gz" },
  "linux-x64": { target: "x86_64-unknown-linux-gnu", ext: "tar.gz" },
  "win32-x64": { target: "x86_64-pc-windows-msvc", ext: "zip" },
};

const key = `${process.platform}-${process.arch}`;
const binaryExt = process.platform === "win32" ? ".exe" : "";
const binDir = path.join(__dirname, "bin");
const infigraphBin = path.join(binDir, `infigraph${binaryExt}`);
const mcpBin = path.join(binDir, `infigraph-mcp${binaryExt}`);

// Already installed?
if (fs.existsSync(infigraphBin)) {
  try {
    const out = execSync(`"${infigraphBin}" --version`, {
      encoding: "utf8",
      timeout: 5000,
    });
    if (out.includes(VERSION)) {
      console.log(`[infigraph] v${VERSION} already installed`);
      registerMcp();
      runMigration();
      process.exit(0);
    }
  } catch (_) {}
}

// Strategy 1: Copy from platform-specific optional dependency
const platformPkg = PLATFORM_PACKAGES[key];
if (platformPkg) {
  try {
    const pkgDir = path.dirname(require.resolve(`${platformPkg}/package.json`));
    const srcBin = path.join(pkgDir, "bin");
    if (fs.existsSync(srcBin)) {
      console.log(`[infigraph] Installing from ${platformPkg}...`);
      copyDirSync(srcBin, binDir);
      setPermissions();
      if (verifyInstall()) {
        registerMcp();
        runMigration();
        process.exit(0);
      }
    }
  } catch (_) {
    // Platform package not installed — fall through to download
  }
}

// Strategy 2: Download from GitHub releases
const platform = PLATFORM_TARGETS[key];
if (!platform) {
  console.error(`[infigraph] Unsupported platform: ${key}`);
  console.error("[infigraph] Supported: macOS (arm64, x64), Linux (x64), Windows (x64)");
  process.exit(1);
}

let baseUrl = `https://github.com/intuit/infigraph/releases/download/v${VERSION}`;
if (process.env.INFIGRAPH_MIRROR) {
  baseUrl = process.env.INFIGRAPH_MIRROR;
}

const archiveName = `infigraph-${platform.target}.${platform.ext}`;
const url = `${baseUrl}/${archiveName}`;
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "infigraph-"));
const tmpArchive = path.join(tmpDir, archiveName);

console.log(`[infigraph] Downloading v${VERSION} for ${platform.target}...`);

try {
  execSync(`curl -fSL --retry 3 "${url}" -o "${tmpArchive}"`, {
    stdio: ["pipe", "pipe", "inherit"],
    timeout: 120000,
  });
} catch (_) {
  console.error(`[infigraph] Download failed from ${url}`);
  console.error("[infigraph] If behind a corporate network, set INFIGRAPH_MIRROR to your Artifactory URL");
  cleanup(tmpDir);
  process.exit(1);
}

console.log("[infigraph] Extracting...");
fs.mkdirSync(binDir, { recursive: true });

try {
  if (platform.ext === "tar.gz") {
    execSync(`tar -xzf "${tmpArchive}" -C "${binDir}"`, { stdio: "pipe" });
  } else {
    execSync(
      `powershell -Command "Expand-Archive -Force -Path '${tmpArchive}' -DestinationPath '${binDir}'"`,
      { stdio: "pipe" }
    );
  }
} catch (e) {
  console.error(`[infigraph] Extraction failed: ${e.message}`);
  cleanup(tmpDir);
  process.exit(1);
}

setPermissions();
verifyInstall();
cleanup(tmpDir);
registerMcp();
runMigration();

// --- Helpers ---

function setPermissions() {
  if (process.platform !== "win32") {
    if (fs.existsSync(infigraphBin)) fs.chmodSync(infigraphBin, 0o755);
    if (fs.existsSync(mcpBin)) fs.chmodSync(mcpBin, 0o755);
  }
}

function verifyInstall() {
  try {
    const ver = execSync(`"${infigraphBin}" --version`, {
      encoding: "utf8",
      timeout: 5000,
    });
    console.log(`[infigraph] Installed: ${ver.trim()}`);
    return true;
  } catch (_) {
    console.error("[infigraph] Installation verification failed");
    return false;
  }
}

function cleanup(dir) {
  try {
    fs.rmSync(dir, { recursive: true, force: true });
  } catch (_) {}
}

function registerMcp() {
  try {
    execSync(`"${infigraphBin}" install`, { stdio: "inherit", timeout: 15000 });
  } catch (e) {
    console.error(`[infigraph] MCP registration failed: ${e.message}`);
  }
}

function runMigration() {
  try {
    require("./migrate.js");
  } catch (_) {}
}

function copyDirSync(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDirSync(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}
