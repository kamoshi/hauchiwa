import { build } from "npm:esbuild@0.27.2";

const contents = `
  export * from "svelte";
  export * from "svelte/internal/client";
  export * from "svelte/internal/flags/legacy";
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
  outfile: "bundle.js",
  sourcemap: "external",
  mainFields: ["svelte", "browser", "module", "main"],
  conditions: ["svelte", "browser", "production"],
});

const script = bundle.outputFiles.find((f) => f.path.endsWith(".js"))!;
const srcmap = bundle.outputFiles.find((f) => f.path.endsWith(".js.map"))!;

const header = new TextEncoder().encode(
  `${script.contents.length} ${srcmap.contents.length}\n`,
);

await Deno.stdout.write(header);
await Deno.stdout.write(script.contents);
await Deno.stdout.write(srcmap.contents);
