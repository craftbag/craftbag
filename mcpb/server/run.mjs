#!/usr/bin/env node
// Starts the PATH-installed craftbag-mcp binary and exits with its code.
// There is no npm package, so a missing binary is a hard error.
import { spawn, execFileSync } from "node:child_process";

const isWin = process.platform === "win32";
const bin = isWin ? "craftbag-mcp.exe" : "craftbag-mcp";

function binaryOnPath(name) {
  try {
    execFileSync(isWin ? "where.exe" : "which", [name], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function run(command, args) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      stdio: "inherit",
      shell: isWin,
      env: process.env,
      windowsHide: true,
    });
    child.on("error", () => resolve(1));
    child.on("exit", (code, signal) => {
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

if (!binaryOnPath(bin)) {
  console.error(
    "craftbag-mcp is not on PATH. Install it with cargo install craftbag-mcp, or from a GitHub Release.",
  );
  process.exit(1);
}

process.exit(await run(bin, process.argv.slice(2)));
