import { build } from "npm:esbuild@0.27.2";
import svelte from "npm:esbuild-svelte@0.9.4";

const file = Deno.args[0];

if (!file) {
  throw new Error("server.ts requires args <file>");
}

const ssr = await build({
  entryPoints: [file],
  format: "esm",
  platform: "node",
  minify: true,
  bundle: true,
  write: false,
  mainFields: ["svelte", "module", "main"],
  conditions: ["svelte", "production"],
  plugins: [
    (svelte as unknown as typeof svelte.default)({
      compilerOptions: {
        generate: "server",
      },
    }),
  ],
});

const text = encodeURIComponent(ssr.outputFiles[0].text);
const out = new TextEncoder().encode(text);
await Deno.stdout.write(out);
