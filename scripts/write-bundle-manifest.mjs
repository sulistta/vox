import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { execFileSync } from "node:child_process";

const bundle = resolve(process.argv[2] ?? "dist/vox-dev");
if (!existsSync(bundle) || !statSync(bundle).isDirectory()) {
  throw new Error(`bundle directory does not exist: ${bundle}`);
}

function filesUnder(directory) {
  const result = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...filesUnder(path));
    else if (entry.isFile() && entry.name !== "manifest.json") result.push(path);
  }
  return result;
}

const files = filesUnder(bundle)
  .sort()
  .map((path) => {
    const bytes = readFileSync(path);
    return {
      path: relative(bundle, path),
      bytes: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
  });

let commit = null;
try {
  commit = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim() || null;
} catch {
  // A source archive may not have a Git checkout. The manifest remains useful.
}

const sourceDateEpoch = Number.parseInt(process.env.SOURCE_DATE_EPOCH ?? "", 10);
const manifest = {
  format: "vox-dev-bundle/v1",
  kind: "development",
  source_commit: commit,
  platform: `${process.platform}-${process.arch}`,
  source_date_epoch: Number.isFinite(sourceDateEpoch) ? sourceDateEpoch : null,
  files,
  note: "manifest.json is excluded from its own file list",
};
writeFileSync(join(bundle, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
