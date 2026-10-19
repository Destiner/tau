# Supplemental native dependency notices

This inventory assembles distribution notices, not legal certification. The
collector is filesystem-only: packaging never fetches arbitrary network text.
All supplemental bytes are pinned by SHA-256 in `provenance.json`.

## Source and identity bindings

- Existing upstream notices and summaries use exact package versions and the
  archive's `.cargo_vcs_info.json` revision. Their public raw URLs contain that
  same immutable revision. Monorepo copies share a directory.
- `binding: registry-checksum` instead binds an exact crates.io package/version,
  Cargo.lock archive checksum, published Cargo SPDX declaration, and expected
  archive VCS revision (explicitly `null` if absent). It does **not** assert that
  the standard text repository's revision is the crate's revision.
- `sourceType: standard-grant` identifies verbatim official SPDX License List
  canonical terms at pinned commit `d46e94e2c78ceede1cfc63cfa0396472d2798d4c`
  (release v3.27.0). These are standard terms identified by the published
  declaration, not recovered crate-specific copyright notices. All template
  placeholders, including `<year>` and `<copyright holders>`, remain verbatim.
- `sourceType: crate-source-excerpt` preserves published source-file copyright
  headers verbatim. It pins the whole source-file SHA-256 as well as excerpt
  SHA-256 and crates.io archive identity; collection checks both the hash and
  exact excerpt membership. Pathfinder geometry's 2019 and 2020 notices are
  retained this way, without borrowing its sibling SIMD crate's notice.

The newer objc2 `LICENSE.md` summaries explicitly incorporate MIT, Zlib and
Apache terms by hyperlink. Their original summaries remain included. The
MIT-only crates include full MIT terms; the trio-licensed crates include the
Apache-2.0 alternative. `incorporatedBy` identifies the summary's published
license link, **not** the download origin of our canonical text. The OSI MIT
page presented a browser challenge during collection; the canonical MIT terms
are preserved from the official SPDX mirror instead, with that mirror's exact
source URL/hash. The Apache link was reachable. No copyright holder/year was
inferred from author metadata or copied from another crate.

For nine older archives with omitted notice files, the published SPDX
expression identifies the included MIT or Apache-2.0 standard terms. Public
upstream inspection found no named notice in the accessible exact trees/tags
for block, malloc_buf, mac, objc_exception and simd_helpers; leak and leaky-cow
have no published version tags or named notices at their accessible repository
roots. Pathfinder geometry's recorded tree was unavailable (HTTP 422), and
seahash's recorded GitLab source was unavailable (HTTP 521); an attempted
GitHub mirror lookup did not resolve that revision either. These limitations
are recorded as `archive-notice-absent-published-SPDX-standard-grant`, not as
proof that no additional upstream notice exists. Known complete terms are
included; no source identity or copyright attribution is fabricated.

The index's `declaredAuthors` is factual Cargo metadata, **not a copyright
owner assertion**. `sourceType` describes evidence provenance, not ownership.
`gapClassifications` distinguishes hyperlink-incorporated terms from older
missing-notice-file fallbacks. Neither classification claims legal clearance
or guarantees the absence of non-provided notices. Unknown/ambiguous SPDX,
wrong version/checksum/revision, altered text and escaped paths fail closed.

The collector always retains notices discovered in the package and the existing
exact-version inventory in `gpui/src/components/diagram/licenses`. Standard
grants are additive and cannot replace those notices, modified EPL sources,
or bundled patches. No source cache was changed.

`preparedSources` records exact upstream archive checksums already pinned in
`gpui/scripts/prepare.ts`. These are **not** checksums of patched trees or
registry checksums from the patched Cargo lock; the index labels them separately.
