#!/usr/bin/env node

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const outDir = resolve(process.cwd(), process.argv[2] ?? "dist/vox-dev");
const outputPath = resolve(outDir, "sbom.cdx.json");
mkdirSync(outDir, { recursive: true });

const componentRef = (type, name, version) => {
  const encodedName = type === "npm" ? name.replace(/^@/, "%40") : name;
  return `pkg:${type}/${encodedName}@${version}`;
};

const npmKey = (key) => {
  const peerSuffix = key.indexOf("(");
  const base = peerSuffix >= 0 ? key.slice(0, peerSuffix) : key;
  const versionSeparator = base.lastIndexOf("@");
  if (versionSeparator <= 0) return null;
  return {
    name: base.slice(0, versionSeparator),
    version: base.slice(versionSeparator + 1),
  };
};

const npmLockComponents = () => {
  const lock = readFileSync(resolve(root, "pnpm-lock.yaml"), "utf8");
  const section = lock.match(/^packages:\n([\s\S]*?)^snapshots:/m)?.[1] ?? "";
  const components = [];

  for (const block of section.split(/\n(?=  (?:'[^']+'|[^'\s].*):\n)/)) {
    const keyMatch = block.match(/^  (.+):\n/);
    if (!keyMatch) continue;
    const key = keyMatch[1].replace(/^'(.*)'$/, "$1");
    const parsed = npmKey(key);
    if (!parsed) continue;
    const integrity = block.match(/integrity:\s+([^}\s]+)/)?.[1];
    components.push({
      type: "library",
      name: parsed.name,
      version: parsed.version,
      purl: componentRef("npm", parsed.name, parsed.version),
      ...(integrity ? { hashes: [{ alg: "SHA-512", content: integrity.replace(/^sha512-/, "") }] } : {}),
      properties: [{ name: "vox:source", value: "pnpm-lock.yaml" }],
    });
  }

  return components;
};

const packageManifests = () => {
  const packages = ["packages/agent-core", "packages/protocol", "packages/provider-adapters"];
  return packages.map((relativePath) => ({
    relativePath,
    manifest: JSON.parse(readFileSync(resolve(root, relativePath, "package.json"), "utf8")),
  }));
};

const cargoMetadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
);

const cargoComponents = cargoMetadata.packages.map((pkg) => {
  const source = pkg.source ?? "workspace";
  const properties = [
    { name: "vox:source", value: source },
    { name: "vox:manifest", value: pkg.manifest_path.replace(`${root}/`, "") },
  ];
  return {
    type: "library",
    name: pkg.name,
    version: pkg.version,
    ...(pkg.license ? { licenses: [{ license: { expression: pkg.license } }] } : {}),
    purl: componentRef("cargo", pkg.name, pkg.version),
    properties,
  };
});

const nodeComponents = [];
const nodeDependencies = [];
const workspaceRefs = new Map();
const manifests = packageManifests();
for (const { manifest } of manifests) {
  workspaceRefs.set(manifest.name, componentRef("npm", manifest.name, manifest.version));
}
for (const { manifest } of manifests) {
  const ref = componentRef("npm", manifest.name, manifest.version);
  workspaceRefs.set(manifest.name, ref);
  nodeComponents.push({
    type: "library",
    name: manifest.name,
    version: manifest.version,
    scope: "required",
    purl: ref,
    properties: [{ name: "vox:source", value: "workspace" }],
  });
  const dependencies = {
    ...(manifest.dependencies ?? {}),
    ...(manifest.optionalDependencies ?? {}),
    ...(manifest.devDependencies ?? {}),
  };
  const refs = [];
  for (const [name, specifier] of Object.entries(dependencies)) {
    if (specifier.startsWith("workspace:")) {
      const workspaceRef = workspaceRefs.get(name);
      if (workspaceRef) refs.push(workspaceRef);
    } else {
      const version = specifier.replace(/^[~^<>= ]+/, "").split(" ")[0];
      if (/^\d/.test(version)) refs.push(componentRef("npm", name, version));
    }
  }
  nodeDependencies.push({ ref, dependsOn: [...new Set(refs)] });
}

const lockComponents = npmLockComponents();
const rootRef = "pkg:generic/vox@0.1.0";
const desktopRef = componentRef("cargo", "vox-desktop", "0.1.0");
const components = [
  ...nodeComponents,
  ...lockComponents.filter((lockComponent) => !nodeComponents.some((node) => node.purl === lockComponent.purl)),
  ...cargoComponents,
];
const dependencies = [
  { ref: rootRef, dependsOn: [desktopRef, ...nodeComponents.map((component) => component.purl)] },
  ...nodeDependencies,
  ...(cargoMetadata.resolve?.nodes ?? []).map((node) => ({
    ref: cargoMetadata.packages.find((pkg) => pkg.id === node.id)
      ? componentRef("cargo", cargoMetadata.packages.find((pkg) => pkg.id === node.id).name, cargoMetadata.packages.find((pkg) => pkg.id === node.id).version)
      : node.id,
    dependsOn: (node.dependencies ?? []).map((dependency) => {
      const pkg = cargoMetadata.packages.find((candidate) => candidate.id === dependency.pkg);
      return pkg ? componentRef("cargo", pkg.name, pkg.version) : dependency.pkg;
    }),
  })),
];

const sbom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: `urn:uuid:${randomUUID()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    component: {
      type: "application",
      name: "vox-development-bundle",
      version: "0.1.0",
      bomRef: rootRef,
      purl: rootRef,
      properties: [
        { name: "vox:purpose", value: "development-bundle" },
        { name: "vox:final-release", value: "false" },
        { name: "vox:commit", value: process.env.VOX_COMMIT ?? "working-tree" },
      ],
    },
    tools: [{ vendor: "Vox", name: "write-bundle-sbom.mjs", version: "0.1.0" }],
  },
  components,
  dependencies,
};

writeFileSync(outputPath, `${JSON.stringify(sbom, null, 2)}\n`);
console.log(`wrote ${outputPath} (${components.length} components)`);
