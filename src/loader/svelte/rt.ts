import { build } from "npm:esbuild@0.27.2";
// Ensure this matches the version used in other functions or relies on the same resolution

// Create a virtual entry point that re-exports Svelte features
// 'hydrate' is the critical one used in compile_svelte_init
const stub = `
  export * from "svelte";
  export * from "svelte/internal/client";
  import "svelte/internal/disclose-version";
`;

const bundle = await build({
  stdin: {
    contents: stub,
    resolveDir: Deno.cwd(), // Resolve from current working directory
    loader: "ts",
  },
  platform: "browser",
  format: "esm",
  bundle: true, // Bundle Svelte into this file
  minify: true,
  write: false,
  // Ensure we use the exact same conditions as the component loader
  mainFields: ["svelte", "browser", "module", "main"],
  conditions: ["svelte", "browser", "production"],
});

const js = new TextEncoder().encode(bundle.outputFiles[0].text);
await Deno.stdout.write(js);
Deno.stdout.close();
