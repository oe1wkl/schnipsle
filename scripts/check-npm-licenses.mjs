#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Fails if a production JavaScript dependency uses a licence incompatible with
// Apache-2.0 / App Store distribution (ADR-0005). Uses `pnpm licenses list`.
import { execFileSync } from "node:child_process";

const forbidden = /\b(A?GPL|LGPL|SSPL|EUPL|CC-BY-NC|BUSL)/i;
const output = execFileSync("pnpm", ["licenses", "list", "--prod", "--json"], {
  encoding: "utf8",
});
const byLicense = JSON.parse(output);
const offenders = Object.entries(byLicense)
  .filter(([license]) => forbidden.test(license))
  .flatMap(([license, packages]) =>
    packages.map((p) => `${p.name}@${p.versions.join(",")} (${license})`),
  );

if (offenders.length > 0) {
  console.error("Incompatible licences in production dependencies:\n  " + offenders.join("\n  "));
  process.exit(1);
}
console.log(`Licences OK (${Object.keys(byLicense).join(", ") || "no production dependencies"})`);
