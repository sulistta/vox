import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const bundle = resolve(process.argv[2] ?? "dist/vox-dev");
const manifestPath = join(bundle, "manifest.json");
if (!existsSync(manifestPath)) throw new Error(`manifest not found: ${manifestPath}`);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
if (manifest.format !== "vox-dev-bundle/v1" || !Array.isArray(manifest.files)) {
  throw new Error("unsupported bundle manifest");
}
for (const entry of manifest.files) {
  const path = join(bundle, entry.path);
  if (!existsSync(path) || !statSync(path).isFile()) throw new Error(`missing bundle file: ${entry.path}`);
  const bytes = readFileSync(path);
  const hash = createHash("sha256").update(bytes).digest("hex");
  if (bytes.length !== entry.bytes || hash !== entry.sha256) throw new Error(`bundle hash mismatch: ${entry.path}`);
}
console.log(`verified ${manifest.files.length} bundle files`);
