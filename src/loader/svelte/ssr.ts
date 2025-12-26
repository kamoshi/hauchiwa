const json = Deno.args[0];
const props = JSON.parse(json);

// Dynamic import from the data URI
const { default: SSR } = await import("data:text/javascript,__PLACEHOLDER__");

let output = null;

if (!output) {
  try {
    const data = { out: [] };
    SSR(data, props);
    output = data.out.join("");
  } catch {}
}

if (!output) {
  try {
    const data = { out: "" };
    SSR(data, props);
    output = data.out;
  } catch {}
}

if (!output) {
  throw "Failed to produce prerendered component, are you using svelte 5?";
}

const html = new TextEncoder().encode(output);
await Deno.stdout.write(html);
Deno.stdout.close();
