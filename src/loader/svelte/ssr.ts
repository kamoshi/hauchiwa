import type { Component } from "npm:svelte@5.46.1";
import { render } from "npm:svelte@5.46.1/server";

const json = Deno.args[0];
const props = JSON.parse(json);

const module = await import("data:text/javascript,__PLACEHOLDER__");
const Comp = module.default as Component;

const rendered = render(Comp, { props });

if (!rendered.body) {
  throw "Failed to produce prerendered component, are you using svelte 5?";
}

const out = new TextEncoder().encode(rendered.body);
await Deno.stdout.write(out);
