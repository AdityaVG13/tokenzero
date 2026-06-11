#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const candidate = resolveTokenZero();

if (!candidate) {
  console.error(
    "tokenzero executable not found; install the Rust CLI or set TOKENZERO_BIN"
  );
  process.exit(127);
}

const result = spawnTokenZero(candidate, process.argv.slice(2), {
  stdio: "inherit",
  shell: false
});

if (result.error) {
  console.error(`tokenzero executable failed: ${result.error.message}`);
  process.exit(127);
}

process.exit(result.status ?? 1);

function findTokenZero() {
  const pathEntries = (process.env.PATH || "")
    .split(path.delimiter)
    .filter(Boolean);
  const names =
    process.platform === "win32"
      ? ["tokenzero.exe", "tokenzero.cmd", "tokenzero.bat"]
      : ["tokenzero"];

  for (const dir of pathEntries) {
    for (const name of names) {
      const candidate = path.join(dir, name);
      if (isSelf(candidate)) {
        continue;
      }
      if (isExecutable(candidate)) {
        return candidate;
      }
    }
  }
  return null;
}

function resolveTokenZero() {
  const configured = process.env.TOKENZERO_BIN;
  if (!configured) {
    return findTokenZero();
  }
  if (isSelf(configured)) {
    console.error("TOKENZERO_BIN points to the npm shim; refusing recursive launch");
    process.exit(127);
  }
  if (!isExecutable(configured)) {
    console.error("TOKENZERO_BIN does not point to an executable tokenzero binary");
    process.exit(127);
  }
  return configured;
}

function spawnTokenZero(candidate, args, options) {
  if (process.platform === "win32" && /\.(cmd|bat)$/i.test(candidate)) {
    return spawnSync("cmd.exe", ["/D", "/C", "call", candidate, ...args], options);
  }
  return spawnSync(candidate, args, options);
}

function isExecutable(candidate) {
  try {
    const stat = fs.statSync(candidate);
    if (!stat.isFile()) {
      return false;
    }
    if (process.platform === "win32") {
      return true;
    }
    fs.accessSync(candidate, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function isSelf(candidate) {
  try {
    const realCandidate = fs.realpathSync(candidate);
    const realSelf = fs.realpathSync(__filename);
    if (realCandidate === realSelf) {
      return true;
    }
  } catch {
    // Fall through to shim-content detection below.
  }
  return isSelfShim(candidate);
}

function isSelfShim(candidate) {
  try {
    const text = fs.readFileSync(candidate, "utf8").slice(0, 8192);
    const normalizedText = normalizePathText(text);
    const normalizedSelf = normalizePathText(__filename);
    return normalizedText.includes(normalizedSelf);
  } catch {
    return false;
  }
}

function normalizePathText(value) {
  return value.replace(/\\/g, "/").toLowerCase();
}
