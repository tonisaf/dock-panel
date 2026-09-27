// Publishes a signed release that installed copies pick up via the updater.
//
//   pnpm release 0.2.0 "What changed"
//
// Bumps the version, builds the signed NSIS installer, commits + tags + pushes,
// then creates a GitHub release in the (private) repo with the installer, its
// signature and latest.json. The updater reads latest.json through the GitHub
// API, so its installer URL is the asset's API URL, not the browser one.
// GitHub calls are retried, and the release stays a draft until it is complete.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

const REPO = "tonisaf/dock-panel";
const KEY_PATH = join(homedir(), ".tauri", "dock-panel.key");

const [version, notes = ""] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error('Usage: pnpm release <x.y.z> ["release notes"]');
  process.exit(1);
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: "inherit", ...opts });
  if (r.status !== 0) {
    console.error(`\n✗ ${cmd} ${args.join(" ")} failed`);
    process.exit(r.status ?? 1);
  }
}

function capture(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")}: ${r.stderr}`);
  return r.stdout.trim();
}

// pnpm on Windows is a .cmd shim, which Node only spawns through a shell (and
// then warns that the args are not escaped). `pnpm release` sets npm_execpath
// to pnpm's own entry point, so run that directly.
function pnpm(args, opts) {
  const entry = process.env.npm_execpath;
  if (entry && /\.[cm]?js$/.test(entry)) run(process.execPath, [entry, ...args], opts);
  else if (entry) run(entry, args, opts);
  else run("pnpm", args, { shell: process.platform === "win32", ...opts });
}

// GitHub calls fail now and then (network, 5xx); every one below is safe to repeat.
function retry(label, fn) {
  for (let attempt = 1; ; attempt++) {
    try {
      return fn();
    } catch (e) {
      if (attempt === 4) {
        console.error(`\n✗ ${label} failed: ${e.message}`);
        process.exit(1);
      }
      console.warn(`… ${label} failed (attempt ${attempt}), retrying`);
      spawnSync(process.execPath, ["-e", `setTimeout(() => {}, ${attempt * 3000})`]);
    }
  }
}

const gh = (args) => capture("gh", [...args, "--repo", REPO]);

// ---- preflight -------------------------------------------------------------------
if (capture("git", ["status", "--porcelain"])) {
  console.error("✗ Working tree is not clean; commit or stash first.");
  process.exit(1);
}
if (capture("git", ["rev-parse", "--abbrev-ref", "HEAD"]) !== "main") {
  console.error("✗ Release from the main branch.");
  process.exit(1);
}
if (!existsSync(KEY_PATH)) {
  console.error(`✗ Signing key not found at ${KEY_PATH}`);
  process.exit(1);
}

// ---- bump version ----------------------------------------------------------------
const pkg = JSON.parse(readFileSync("package.json", "utf8"));
pkg.version = version;
writeFileSync("package.json", JSON.stringify(pkg, null, 2) + "\n");

const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
conf.version = version;
writeFileSync("src-tauri/tauri.conf.json", JSON.stringify(conf, null, 2) + "\n");

const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
writeFileSync("src-tauri/Cargo.toml", cargo.replace(/^version = "[^"]+"/m, `version = "${version}"`));

// ---- build (signed) --------------------------------------------------------------
pnpm(["tauri", "build"], {
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(KEY_PATH, "utf8"),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
  },
});

const bundleDir = "src-tauri/target/release/bundle/nsis";
const installer = join(bundleDir, `${conf.productName}_${version}_x64-setup.exe`);
const signature = `${installer}.sig`;
for (const f of [installer, signature]) {
  if (!existsSync(f)) {
    console.error(`✗ Missing build output: ${f}`);
    process.exit(1);
  }
}

// ---- commit, tag, push -----------------------------------------------------------
const tag = `v${version}`;
run("git", ["add", "package.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"]);
run("git", ["commit", "-m", `Release ${tag}`]);
run("git", ["tag", "-a", tag, "-m", `Dock Panel ${version}`]);
retry("git push", () => capture("git", ["push", "--follow-tags"]));

// ---- GitHub release --------------------------------------------------------------
// Created as a draft and published only once latest.json is there: installed
// copies look at the latest published release, and one without the manifest
// would look broken to them.
const exists = () => {
  try {
    gh(["release", "view", tag, "--json", "tagName"]);
    return true;
  } catch {
    return false;
  }
};
retry("create release", () => {
  if (!exists()) {
    gh(["release", "create", tag, "--draft", "--verify-tag", "--title", `Dock Panel ${version}`, "--notes", notes || `Dock Panel ${version}`]);
  }
});
retry("upload installer", () => gh(["release", "upload", tag, installer, signature, "--clobber"]));

const assets = retry("read release", () => JSON.parse(gh(["release", "view", tag, "--json", "assets"])).assets);
const asset = assets.find((a) => a.name.endsWith(`_${version}_x64-setup.exe`));
if (!asset) {
  console.error("✗ Installer asset not found in the release");
  process.exit(1);
}

const platform = { signature: readFileSync(signature, "utf8").trim(), url: asset.apiUrl };
const manifest = {
  version,
  notes: notes || undefined,
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": platform, "windows-x86_64-nsis": platform },
};
const manifestPath = join(mkdtempSync(join(tmpdir(), "dock-panel-")), "latest.json");
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
retry("upload latest.json", () => gh(["release", "upload", tag, manifestPath, "--clobber"]));
retry("publish release", () => gh(["release", "edit", tag, "--draft=false", "--latest"]));

console.log(`\n✓ Released ${tag}: https://github.com/${REPO}/releases/tag/${tag}`);
