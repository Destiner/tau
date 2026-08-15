export default {
  extends: [
    'stylelint-config-standard',
    'stylelint-config-recommended-vue',
    'stylelint-config-property-sort-order-smacss',
  ],
  rules: {
    'media-feature-name-allowed-list': [
      'width',
      'prefers-color-scheme',
      'prefers-reduced-motion',
    ],
    // The stylesheet is grouped by component, so base rules follow the
    // contextual variants that override them. Satisfying this rule would mean
    // reordering the cascade rather than changing what any of it does.
    'no-descending-specificity': null,
  },
};
