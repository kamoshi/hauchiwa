import { build } from "npm:esbuild@0.27.2";

const contents = `
  export * from "svelte";
  export * from "svelte/internal/client";
  import "svelte/internal/disclose-version";
`;

const bundle = await build({
  stdin: {
    contents,
    resolveDir: Deno.cwd(),
    loader: "ts",
  },
  platform: "browser",
  format: "esm",
  bundle: true,
  minify: true,
  write: false,
  mainFields: ["svelte", "browser", "module", "main"],
  conditions: ["svelte", "browser", "production"],
});

const out = new TextEncoder().encode(bundle.outputFiles[0].text);
await Deno.stdout.write(out);
