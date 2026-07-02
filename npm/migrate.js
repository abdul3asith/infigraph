#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const os = require("os");
const { execSync } = require("child_process");

const home = os.homedir();

// ==================== Step 1: Uninstall terragraph ====================

// Remove terragraph from DevAssist plugins
const lockPaths = [
  path.join(home, ".claude", "devassist-plugins.lock.json"),
  path.join(home, ".cursor", "devassist-plugins.lock.json"),
];

for (const lockFile of lockPaths) {
  if (fs.existsSync(lockFile)) {
    try {
      const lock = JSON.parse(fs.readFileSync(lockFile, "utf8"));
      if (lock["terragraph-plugin"] || lock["terragraph"]) {
        console.log("[infigraph] Removing terragraph from DevAssist plugins...");
        try {
          execSync(
            'npx --registry https://registry.npmjs.intuit.com @dev-platformexps/devassist-plugin-manager@latest remove terragraph-plugin',
            { stdio: "pipe", timeout: 30000 }
          );
          console.log("[infigraph] Terragraph plugin removed from DevAssist");
        } catch (_) {
          // Manual cleanup if plugin manager fails
          delete lock["terragraph-plugin"];
          delete lock["terragraph"];
          fs.writeFileSync(lockFile, JSON.stringify(lock, null, 2));
          console.log("[infigraph] Terragraph entry cleaned from lock file");
        }
      }
    } catch (_) {}
  }
}

// Remove terragraph binary from common locations
const oldBinaries = [
  path.join(home, ".local", "bin", "terragraph"),
  path.join(home, ".local", "bin", "terragraph-mcp"),
  path.join(home, ".cargo", "bin", "terragraph"),
  path.join(home, ".cargo", "bin", "terragraph-mcp"),
];

for (const bin of oldBinaries) {
  if (fs.existsSync(bin)) {
    try {
      fs.unlinkSync(bin);
      console.log(`[infigraph] Removed ${bin}`);
    } catch (_) {}
  }
}

// Windows .exe variants
if (process.platform === "win32") {
  for (const bin of oldBinaries) {
    const exe = bin + ".exe";
    if (fs.existsSync(exe)) {
      try {
        fs.unlinkSync(exe);
        console.log(`[infigraph] Removed ${exe}`);
      } catch (_) {}
    }
  }
}

// Remove terragraph MCP from Claude Desktop config
const claudeConfigs = [
  path.join(home, "Library", "Application Support", "Claude", "claude_desktop_config.json"),
  path.join(home, "AppData", "Roaming", "Claude", "claude_desktop_config.json"),
  path.join(home, ".config", "Claude", "claude_desktop_config.json"),
];

for (const configPath of claudeConfigs) {
  if (fs.existsSync(configPath)) {
    try {
      const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
      if (config.mcpServers && config.mcpServers.terragraph) {
        delete config.mcpServers.terragraph;
        fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
        console.log("[infigraph] Removed terragraph from Claude MCP config");
      }
    } catch (_) {}
  }
}

// Remove terragraph from Claude Code MCP settings
const claudeCodeSettings = path.join(home, ".claude", "settings.json");
if (fs.existsSync(claudeCodeSettings)) {
  try {
    const settings = JSON.parse(fs.readFileSync(claudeCodeSettings, "utf8"));
    let changed = false;
    if (settings.mcpServers && settings.mcpServers.terragraph) {
      delete settings.mcpServers.terragraph;
      changed = true;
    }
    if (changed) {
      fs.writeFileSync(claudeCodeSettings, JSON.stringify(settings, null, 2));
      console.log("[infigraph] Removed terragraph from Claude Code settings");
    }
  } catch (_) {}
}

// Remove terragraph skills/commands
const oldSkillDirs = [
  path.join(home, ".claude", "commands", "terragraph"),
  path.join(home, ".cursor", "commands", "terragraph"),
  path.join(home, ".claude", "skills", "terragraph"),
];

for (const dir of oldSkillDirs) {
  if (fs.existsSync(dir)) {
    try {
      fs.rmSync(dir, { recursive: true, force: true });
      console.log(`[infigraph] Removed ${dir}`);
    } catch (_) {}
  }
}

// ==================== Step 2: Migrate data ====================

// Global config: ~/.terragraph → ~/.infigraph
const oldGlobal = path.join(home, ".terragraph");
const newGlobal = path.join(home, ".infigraph");

if (fs.existsSync(oldGlobal) && !fs.existsSync(newGlobal)) {
  try {
    copyDirSync(oldGlobal, newGlobal);
    console.log(`[infigraph] Migrated ${oldGlobal} → ${newGlobal}`);
    console.log("[infigraph] Old .terragraph/ kept as backup — delete when ready");
  } catch (e) {
    console.error(`[infigraph] Global migration failed: ${e.message}`);
    console.error(`[infigraph] Manually copy ${oldGlobal} to ${newGlobal}`);
  }
}

// Scan common project directories for .terragraph/ folders
const projectRoots = [
  path.join(home, "SourceCode"),
  path.join(home, "src"),
  path.join(home, "projects"),
  path.join(home, "repos"),
  path.join(home, "code"),
  path.join(home, "workspace"),
  path.join(home, "dev"),
];

let projectsMigrated = 0;

for (const root of projectRoots) {
  if (!fs.existsSync(root)) continue;
  try {
    const entries = fs.readdirSync(root, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const oldDir = path.join(root, entry.name, ".terragraph");
      const newDir = path.join(root, entry.name, ".infigraph");
      if (fs.existsSync(oldDir) && !fs.existsSync(newDir)) {
        try {
          copyDirSync(oldDir, newDir);
          projectsMigrated++;
        } catch (_) {}
      }
    }
  } catch (_) {}
}

if (projectsMigrated > 0) {
  console.log(`[infigraph] Migrated ${projectsMigrated} project(s) from .terragraph/ → .infigraph/`);
}

// ==================== Done ====================

console.log("[infigraph] Migration complete.");

// ==================== Helpers ====================

function copyDirSync(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  const entries = fs.readdirSync(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDirSync(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}
