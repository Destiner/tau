Ayu Dark and Ayu Light token colors, sourced from the installed `@shikijs/themes` package (`@shikijs/themes/ayu-dark` and `@shikijs/themes/ayu-light`, version 4.4.3). The bundled JSON retains the original `name`, `editor.foreground`, `editor.background`, and full `tokenColors` list; editor UI colors and semantic token colors are omitted because Syntect operates on TextMate grammar scopes. See LICENSE (package license). Regenerate from the installed package exports, not a hand-written approximation.

## Grammar set

`components/syntax.rs` loads `two_face::syntax::extra_newlines()` once. The
crates.io dependency is pinned to `two-face = 0.4.5`, with default features off
and only `syntect-fancy` enabled, matching the existing Syntect 5.3.0
`default-fancy` backend. No Oniguruma/native regex dependency is added. The
crate depends only on the already-locked serde, serde_derive and syntect; no
existing package versions need to change.

This is the upstream precompiled newline-aware grammar set curated by bat
(two-face 0.4.5 follows bat 0.25.0), not a local grammar or a Shiki scope
approximation. It supplies real grammars for all 15 production languages:
Bash, CSS, Diff, Go, HTML, JavaScript, JSON, Markdown, Python, Rust, TOML,
TypeScript, TSX (`TypescriptReact`), Vue and YAML. Production `jsx` fences
continue to normalize to `tsx`. TypeScript/TSX no longer use JavaScript and Vue
no longer uses HTML as a fallback. Grammar token classification can differ
from Shiki, but supported TOML is not rendered wholly plain.

A standalone TOML asset would be smaller, but would leave TypeScript/TSX and
Vue on partial fallback grammars. The chosen fancy-newlines payload is about
900 KiB (upstream estimates about 0.5 MiB more than Syntect defaults). Only the
syntax loader is called; two-face's themes are not used. The existing Ayu
JSON remains the token source. RGBA conversion preserves each token's alpha
channel, including transparent and partially transparent colors. Bold and italic
map to native font styles; underline now maps to a native 1px, non-wavy underline
using the token color, including its alpha. Syntax mapping regressions cover
alpha values 0, 128 and 255 and the absence of underline on plain tokens.

Provenance and notices:

- Source: <https://github.com/CosmicHorrorDev/two-face/tree/v0.4.5>
- crates.io checksum (also in Cargo.lock):
  `39e51b6e60e545cfdae5a4639ff423818f52372211a8d9a3e892b4b0761f76b2`
- Crate license: MIT OR Apache-2.0; exact shipped license texts are preserved
  as `two-face-0.4.5-LICENSE-MIT` and `two-face-0.4.5-LICENSE-APACHE`.
- Embedded grammars have separate upstream licenses. The complete upstream
  notices, including bat's notice and grammar copyright/license texts, are
  preserved verbatim in `two-face-0.4.5-ACKNOWLEDGEMENTS.md`, from
  <https://raw.githubusercontent.com/CosmicHorrorDev/two-face/v0.4.5/generated/acknowledgements_full.md>.
  SHA-256: `55be6f98f911df5cecc95275d95b75a916c3f4efb5f0dd322c7f835a60aedeab`.
  In particular, TOML is MIT (Jason Williams), TypeScript/TSX is Apache-2.0,
  and Vue is MIT (Evan You). The complete inventory also records the Sublime,
  BSD and other licenses for the rest of the bundled set; the crate's license
  alone is not a substitute for these grammar notices.
