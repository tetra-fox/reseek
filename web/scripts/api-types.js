// Writes src/lib/api/schema.d.ts from the committed ../api/openapi.json. With `--check` it writes
// nothing and fails when the file is not what it would write.
//
// openapi-typescript refuses two things the daemon's document has. It names some query parameters'
// enums by reference without listing them under components.schemas (utoipa leaves out enums that
// only appear in an IntoParams struct); each missing schema is filled in as a plain string, the
// type every one of them serialises as. And several operations share an operationId (utoipa
// takes the handler's function name, and handlers in different modules share names); the ids are
// dropped, so every operation's types are written inline under its path, which is all
// openapi-fetch reads.
// TODO: remove both once the daemon's document lists those enums and has unique operationIds.
import { existsSync, readFileSync, writeFileSync } from "node:fs";

import openapiTS, { astToString, COMMENT_HEADER } from "openapi-typescript";

const source = new URL("../../api/openapi.json", import.meta.url);
const target = new URL("../src/lib/api/schema.d.ts", import.meta.url);

const document = JSON.parse(readFileSync(source, "utf8"));
const schemas = (document.components ??= {}).schemas ?? {};

/** Every `#/components/schemas/<name>` the document refers to */
const referenced = (value, found = new Set()) => {
  if (Array.isArray(value)) value.forEach((v) => referenced(v, found));
  else if (value && typeof value === "object") {
    for (const [key, v] of Object.entries(value)) {
      if (key === "$ref" && typeof v === "string" && v.startsWith("#/components/schemas/")) {
        found.add(v.slice("#/components/schemas/".length));
      } else referenced(v, found);
    }
  }
  return found;
};

const missing = [...referenced(document)].filter((name) => !(name in schemas)).sort();
for (const name of missing) {
  schemas[name] = { type: "string", description: "not listed in the document, read as a string" };
}
document.components.schemas = schemas;
if (missing.length && !process.argv.includes("--check")) {
  process.stderr.write(
    `schemas missing from openapi.json, typed as string: ${missing.join(", ")}\n`
  );
}

for (const item of Object.values(document.paths ?? {})) {
  for (const operation of Object.values(item)) {
    if (operation && typeof operation === "object") delete operation.operationId;
  }
}

const text = COMMENT_HEADER + astToString(await openapiTS(document, { silent: true }));
if (!process.argv.includes("--check")) {
  writeFileSync(target, text);
} else if (!existsSync(target) || readFileSync(target, "utf8") !== text) {
  process.stderr.write("src/lib/api/schema.d.ts is stale, run `just api`\n");
  process.exit(1);
}
