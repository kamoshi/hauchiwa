import { dirname, resolve } from "node:path";
import { build } from "npm:esbuild@0.27.2";
import svelte from "npm:esbuild-svelte@0.9.4";

const file = Deno.args[0];
const hash = Deno.args[1];

if (!file || !hash) {
  throw new Error("init.ts requires args <file> <hash>");
}

const contents = `
  import { hydrate } from "svelte";
  import App from ${JSON.stringify(file)};

  const query = document.querySelectorAll('._${hash}');
  for (const target of query) {
    const attrs = target.getAttribute('data-props');
    const props = JSON.parse(attrs) ?? {};
    hydrate(App, { target, props });
  }
`;

const ssr = await build({
  stdin: {
    contents,
    resolveDir: dirname(resolve(file)),
    sourcefile: "__virtual.ts",
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
  external: ["svelte"],
  plugins: [
    (svelte as unknown as typeof svelte.default)({
      compilerOptions: {
        css: "external",
      },
    }),
  ],
});

const script = ssr.outputFiles.find((f) => f.path.endsWith(".js"))!;
const srcmap = ssr.outputFiles.find((f) => f.path.endsWith(".js.map"))!;

const header = new TextEncoder().encode(
  `${script.contents.length} ${srcmap.contents.length}\n`,
);

await Deno.stdout.write(header);
await Deno.stdout.write(script.contents);
await Deno.stdout.write(srcmap.contents);
