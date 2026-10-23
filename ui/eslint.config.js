// Minimal lint: ESLint core recommended plus typescript-eslint recommended, with two project rules.
// Generated files are excluded. No other plugins.
import js from '@eslint/js';
import tseslint from 'typescript-eslint';

const HTML_SINKS = [
  'MemberExpression[property.name=/^(innerHTML|outerHTML)$/]',
  'MemberExpression[property.name=/^insertAdjacent(HTML|Element)$/]',
  'MemberExpression[property.name="write"][object.name="document"]',
  'JSXAttribute[name.name="dangerouslySetInnerHTML"]',
];

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', 'src/generated/**', 'coverage/**'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx,js,mjs}'],
    rules: {
      '@typescript-eslint/consistent-type-imports': 'error',
    },
  },
  {
    // Page titles and URLs are untrusted. Sidebar code renders them as text only.
    files: ['src/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-syntax': [
        'error',
        ...HTML_SINKS.map((selector) => ({
          selector,
          message: 'Untrusted text must be rendered as text. HTML injection sinks are not allowed in src/.',
        })),
      ],
    },
  },
  {
    files: ['scripts/**/*.mjs'],
    languageOptions: {
      globals: { console: 'readonly', process: 'readonly', URL: 'readonly' },
    },
  },
);
