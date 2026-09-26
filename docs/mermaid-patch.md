# beautiful-mermaid parser patch

Tau pins beautiful-mermaid 1.1.3 and applies `patches/beautiful-mermaid@1.1.3.patch`. Its flowchart parser previously terminated quoted labels at interior shape closers, kept placeholder labels/shapes after later definitions, and sometimes returned partial graphs for malformed statements. See [upstream issue #125](https://github.com/lukilabs/beautiful-mermaid/issues/125).

The package exports `src/index.ts` to Bun and `dist/index.js` to ESM/Vite. Keep both parser implementations in the patch equivalent. After editing the installed package, run `bun patch --commit node_modules/beautiful-mermaid`, then `bun install --frozen-lockfile`. Check both entry points with `bun -e 'import { parseMermaid } from "beautiful-mermaid"; console.log(parseMermaid("graph TD\n A --> B{\"label []\"}").nodes.get("B"))'` and the same command with `node --input-type=module -e`. Run `bunx vitest run src/lib/mermaid-parser.test.ts src/lib/mermaid-source.test.ts src/lib/mermaid.test.ts` and the transcript browser tests.

Remove the patch only when a released upstream version passes the same regressions in both entry points and the UI. The patch tightens Tau's supported flowchart subset; it does not implement the entire Mermaid grammar.
