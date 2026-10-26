/** A config value as a Nix expression, for pasting into a NixOS module's settings */
export const toNix = (value: unknown, indent = ""): string => {
  if (typeof value === "string") {
    // JSON's escapes are Nix's, apart from ${ starting an interpolation
    return JSON.stringify(value).replaceAll("${", "\\${");
  }
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) {
    return value.length ? `[ ${value.map((v) => toNix(v, indent)).join(" ")} ]` : "[ ]";
  }
  if (value && typeof value === "object") {
    const inner = `${indent}  `;
    const attrs = Object.entries(value).map(
      ([k, v]) =>
        `${inner}${/^[A-Za-z_][A-Za-z0-9_'-]*$/.test(k) ? k : JSON.stringify(k)} = ${toNix(v, inner)};`
    );
    return `{\n${attrs.join("\n")}\n${indent}}`;
  }
  return "null";
};

/** Dotted keys and their values as nested attrs, `web.port` as `{ web = { port = 5030; }; }` */
export const nest = (entries: [string, unknown][]) => {
  const root: Record<string, unknown> = {};
  for (const [key, value] of entries) {
    const parts = key.split(".");
    let at = root;
    for (const part of parts.slice(0, -1)) {
      at = (at[part] ??= {}) as Record<string, unknown>;
    }
    at[parts[parts.length - 1]] = value;
  }
  return root;
};

/** The module setting that carries `entries`, ready to paste */
export const asNixSettings = (entries: [string, unknown][]) =>
  `services.reseek.settings = ${toNix(nest(entries))};\n`;
