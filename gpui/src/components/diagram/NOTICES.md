# Native diagram third-party notices

This module is **not MIT-only**. Its Rust dependency inventory (including shared
host-enabled features and build dependencies) is recorded in
[LICENSE-INVENTORY.md](LICENSE-INVENTORY.md). Versions/checksums are pinned by
`gpui/Cargo.lock`. Original-release licence and copyright texts for the inventory are
retained in `licenses/` as provenance; added EPL-covered files are separately
licensed and do not alter those original texts; packaging must include these notices/texts with any
binary distribution. The inventory is a conservative dependency-closure notice,
not a claim that every optional parser/layout family is compiled. The recorded
188-package inventory covers the diagram dependency closure, not the entire
application lockfile. It is not a binary-bundled inventory or certification of
legal compliance. Reconcile the closure, enabled features, notices and source
availability whenever dependencies change and before distribution.

## Direct components

- merman 0.8.0 and merman-core 0.8.0: MIT OR Apache-2.0.
  The currently patched merman-render 0.8.0 is exposed under MIT; this does not
  override separately attributed components or the EPL-covered compactor.
  Release commit `2029bd547077b1a6800878e125663633d81e6d87`.
  Source: https://github.com/Latias94/merman.
  merman archive SHA256:
  `e01ae09c1f8c131667a3a8f95e395ca089a7d41f6551e068ab3e8dc158b3b9fb`;
  core `c8375d78a1c09f5ad1c847d8d18658f5dbe4221c31b9eb56d4a3dc121184f9d4`;
  render `bbb640e1bb9e614384724f9f9efae431a6faf464c34b95e74d56200fcbfd6c7b`.
- resvg/usvg 0.47.0: Apache-2.0 OR MIT.
  https://github.com/linebender/resvg.
- roxmltree 0.21.1: MIT OR Apache-2.0.
  https://github.com/RazrFalcon/roxmltree.
- rustybuzz 0.20.1: MIT; copyright HarfBuzz developers and Yevhenii Reizner.
  https://github.com/harfbuzz/rustybuzz.
- XY layout constants, tick algorithm, legend layout, rounded bars, natural
  cubic spline and colour-tier formula adapted in `xy.rs`/`presentation.rs` from
  beautiful-mermaid 1.1.3: MIT. https://github.com/lukilabs/beautiful-mermaid.
  Original copyright/licence: `licenses/beautiful-mermaid-1.1.3/LICENSE`.
  This is a Rust port of generic algorithms, not bundled JavaScript or assets.

## Local compatibility modifications

The official preparation script verifies and patches `merman-core` and
`merman-analysis` into `.cache/crates/`. Analysis archive SHA256:
`9359567480a01600cfe8f17811ffcf7d51df17697e95f6e37bd1ba7d5aefbdde`.
The root `gpui/scripts/prepare.ts` verifies archives and applies the focused
patches in `gpui/patches/`; root Cargo.lock owns resolution. Original
Cargo.toml.orig remains as provenance. The core/analysis parser-version patches
raise granit-parser and serde-saphyr pins from 1.0.0 to 1.3.0 to avoid downgrading
existing i18n/parser dependencies. Core also changes ER's default semantic
direction to LR (explicit directions still win). Four registered render patches
provide opt-in class/ER, state/sequence, flowchart geometry/text roles/markers/SVG
presentation, and renderer polish; the layout-ELK patch exposes adapter profile
options. These are real renderer/layout/semantic changes, not manifest-only work.
Native post-compaction and layer alignment are wired through the official
preparation path. Profile maintenance requires visual review; this notice does not claim
visual parity or legal certification.

## Copied/transformed upstream works inside merman

The upstream core notices and legal files are retained under
`licenses/merman-core-0.8.0/THIRD_PARTY_NOTICES.md` and its `THIRD_PARTY_LICENSES/`.
The renderer's original notices/texts are retained under
`licenses/merman-render-0.8.0/`.
These identify Mermaid 12.1.0 (MIT), DOMPurify 3.4.16 (Apache-2.0 option selected
by merman), sanitize-url 7.1.1 (MIT), d3-shape 3.2.0 (ISC), rough-rs/Rough.js (MIT),
and other upstream family references. Venn/ZenUML features are not selected;
retain the original upstream notices rather than deleting portions of them.
The `layout-elk` feature selects merman-layout-elk 0.8.0 (MIT OR Apache-2.0)
and merman-elk-layered 0.8.0 (**EPL-2.0**), a pure-Rust ELK layered source port.
No math-font exports are selected. Immutable corresponding sources:
https://crates.io/crates/merman-layout-elk/0.8.0 and
https://crates.io/crates/merman-elk-layered/0.8.0. Archive SHA256 values:
layout `4ce75a18461574fd6a20c13dafdc0f865250ad4423e13d1274eff0f41295d8b0`;
layered `7e78d69ce6979bc6df85d9a62baaf3069c2fb44ade7e1a20ec50568c31c73f9d`.
Preserve the EPL notice/text and provide the corresponding EPL-covered source
when distributing binaries. This does not make the whole application EPL;
release packaging/legal review must satisfy the actual EPL obligations.
The opt-in family profiles and algorithm ports derive from beautiful-mermaid
1.1.3 under MIT; the source licence is included in the patches as well as retained
in the notices. Registered renderer profiles are installed by preparation.
The flowchart post-compactor derives from ELK and is EPL-2.0, with
`ELK-COMPACTION-LICENSE.md` and modified corresponding Rust source included in its
patch; it must not be relabelled MIT. Its alignment/bundling ports retain
`BEAUTIFUL-MERMAID-LICENSE.md`. Preserve these attributions and corresponding
source when distributing binaries. Corresponding source distribution must
include the exact upstream release sources plus the checked-in patches needed
to reconstruct the modified EPL-covered compactor and layered engine; an MIT-only
notice or an unmodified upstream source link is not sufficient for modified files.
Final packaging/legal review must verify these obligations.

## Non-permissive and attribution-bearing dependency paths

- cssparser 0.36.0 / 0.38.0, cssparser-macros 0.6.1 / 0.7.1,
  dtoa-short 0.3.5 and selectors 0.37.0: **MPL-2.0**. Preserve their MPL texts
  and make the corresponding source available when distributing executable
  form. They are used unmodified; immutable crates.io sources for the exact
  inventory versions are the corresponding source. MPL is file-level, not a
  statement that all of Tau must become MPL.
- tiny-skia/tiny-skia-path: BSD-3-Clause; arrayref: BSD-2-Clause;
  lol_html: BSD-3-Clause; Rustybuzz carries HarfBuzz attribution.
- Unicode property/data crates: Unicode-3.0; encoding_rs includes an additional
  BSD-3-Clause component; keep these combined notices, not only a chosen MIT text.

This is engineering attribution and source provenance, not legal advice or a
replacement for release packaging/legal review. The parent app must wire notice
inclusion into its distribution; this module does not edit packaging files.

## Font assets

Existing bundled TauInter static instances derive from Inter, copyright 2016
The Inter Project Authors, under **SIL Open Font License 1.1**. Existing texts:
`gpui/assets/Inter-LICENSE.txt` and `gpui/assets/product-fonts/OFL.txt`.
The module changes no font assets. Local macOS Menlo/Arial/Arial Unicode fonts remain
Apple-supplied OS resources; no font bytes are added to the repository or fetched.
They must not be redistributed as part of these notices or app assets.
