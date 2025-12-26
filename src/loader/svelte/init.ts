import * as path from "node:path";
import { build } from "npm:esbuild@0.27.2";
import svelte from "npm:esbuild-svelte@0.9.4";

const file = Deno.args[0];
const hash = Deno.args[1];

const stub = `
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
    contents: stub,
    resolveDir: path.dirname(path.resolve(file)),
    sourcefile: "__virtual.ts",
    loader: "ts",
  },
  platform: "browser",
  format: "esm",
  bundle: true,
  minify: true,
  write: false,
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

const js = new TextEncoder().encode(ssr.outputFiles[0].text);
await Deno.stdout.write(js);
Deno.stdout.close();
