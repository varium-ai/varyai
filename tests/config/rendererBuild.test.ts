import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { beforeAll, describe, expect, it } from "vitest";

it.each(["\0commonjsHelpers.js", "\0vite/preload-helper.js"])(
  "keeps shared runtime helper %s in a dependency-only chunk",
  async (id) => {
    // Load the composite Node config in its own realm, without importing its
    // source into the frontend TypeScript project or requiring emitted files.
    const { stdout } = await promisify(execFile)(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
          import { loadConfigFromFile } from "vite";
          const loaded = await loadConfigFromFile(
            { command: "build", mode: "production" }, "vite.config.ts"
          );
          if (!loaded) throw new Error("Vite config was not loaded");
          const manualChunks = loaded.config.build.rollupOptions.output.manualChunks;
          console.log(JSON.stringify(manualChunks(${JSON.stringify(id)})));
        `,
      ],
      { cwd: process.cwd(), timeout: 30_000, maxBuffer: 1024 * 1024 },
    );
    expect(JSON.parse(stdout)).toBe("vendor-interop");
  },
  30_000,
);

interface ChunkReport {
  file: string;
  bytes: number;
  entry: boolean;
  imports: string[];
  dynamicImports: string[];
}

// Vite/esbuild need Node's own Uint8Array realm, not jsdom's. Build in a
// subprocess without writing dist or launching Tauri; inspect real output.
describe("renderer production build", () => {
  let chunks: ChunkReport[];
  let warningLimit: number;
  let plugins: string[];
  let diagnostics: { level: string; message: string }[];

  beforeAll(async () => {
    const { stdout } = await promisify(execFile)(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
          import { build, createLogger, resolveConfig } from "vite";
          const diagnostics = [];
          // Capture actual logger calls before logLevel filtering; "silent"
          // keeps stdout JSON-only, but must not hide unknown options/warnings.
          const logger = createLogger("silent");
          for (const level of ["warn", "warnOnce", "error"]) {
            logger[level] = message => diagnostics.push({ level, message });
          }
          const config = await resolveConfig({ logLevel: "silent" }, "build");
          const result = await build({
            logLevel: "silent",
            customLogger: logger,
            build: { write: false },
          });
          const outputs = Array.isArray(result) ? result : [result];
          const chunks = outputs.flatMap(output => output.output
            .filter(item => item.type === "chunk")
            .map(chunk => ({
              file: chunk.fileName,
              bytes: Buffer.byteLength(chunk.code, "utf8"),
              entry: chunk.isEntry,
              imports: chunk.imports,
              dynamicImports: chunk.dynamicImports,
            })));
          console.log(JSON.stringify({
            chunks,
            warningLimit: config.build.chunkSizeWarningLimit,
            plugins: config.plugins.map(plugin => plugin.name),
            diagnostics,
          }));
        `,
      ],
      {
        cwd: process.cwd(),
        env: { ...process.env, NODE_ENV: "production" },
        timeout: 60_000,
        maxBuffer: 1024 * 1024,
      },
    );
    ({ chunks, warningLimit, plugins, diagnostics } = JSON.parse(stdout));
  }, 60_000);

  it("accepts build options without hidden warnings or errors", () => {
    expect(diagnostics).toEqual([]);
  });

  it("keeps default size warnings enabled and the inspector development-only", () => {
    expect(warningLimit).toBe(500);
    expect(plugins.some((name) => /inspector/i.test(name))).toBe(false);
  });

  it("emits bounded chunks with resolvable imports and no static cycles", () => {
    expect(chunks.some((chunk) => chunk.entry)).toBe(true);
    expect(chunks.filter((chunk) => chunk.bytes > 500_000)).toEqual([]);

    const graph = new Map(chunks.map((chunk) => [chunk.file, chunk]));
    for (const chunk of chunks) {
      for (const imported of [...chunk.imports, ...chunk.dynamicImports]) {
        expect(graph.has(imported), `${chunk.file} -> ${imported}`).toBe(true);
      }
    }

    const visited = new Set<string>();
    const stack: string[] = [];
    const cycles: string[][] = [];
    const visit = (file: string) => {
      const start = stack.indexOf(file);
      if (start !== -1) {
        cycles.push([...stack.slice(start), file]);
        return;
      }
      if (visited.has(file)) return;
      visited.add(file);
      stack.push(file);
      for (const imported of graph.get(file)!.imports) visit(imported);
      stack.pop();
    };
    for (const file of graph.keys()) visit(file);
    expect(cycles).toEqual([]);
  });
});
