# acorn-web

Firefox `moz-*` web components, extracted and transformed for use outside Firefox.


## Usage

### Import everything

Registers all components at once:

```js
import 'acorn-web';
```

### Import a single component

```js
import 'acorn-web/components/moz-button';
```

### HTML

Components render with English strings by default — no setup required:

```html
<moz-button label="Click me"></moz-button>

<moz-checkbox label="Enable feature" checked></moz-checkbox>

<moz-toggle label="Dark mode"></moz-toggle>

<moz-message-bar type="warning" message="This action cannot be undone."></moz-message-bar>

<moz-card>
  <span slot="header">Card title</span>
  Card body content.
</moz-card>
```

### Global styles

Import shared Firefox design-system styles (tokens, resets, typography):

```js
import 'acorn-web/styles/common-shared.css';
```

### Localization

- **English (default)**: Components work out of the box — aria-labels, titles, and alt text are all present.
- **Fluent (optional)**: Install `@fluent/bundle` and `@fluent/dom`, then call `initFluent()`. All `data-l10n-id` elements translate automatically. FTL files ship at `dist/locales/en-US/`.
- **Custom i18n**: Set up your own `document.l10n` implementation, or set attributes (`title`, `aria-label`, `alt`) directly on component host elements.

### Vite

No special configuration required — acorn-web ships ES modules. Assets (icons, images) are at `dist/assets/` and referenced via relative URLs inside the package.

---

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (for the transformation tool)
- Node.js >= 18 (for Storybook)
- A local Firefox source checkout

### Build components from Firefox source

```bash
export FIREFOX_ROOT=/path/to/firefox
npm run build
```

Or run directly:

```bash
cargo run --release -- /path/to/firefox ./dist ./config.toml
```

### View in Storybook

```bash
npm install
npm run storybook
```

Storybook runs at http://localhost:6006.

### Run Rust tests

```bash
cargo test
```

---

## License

[MPL-2.0](https://opensource.org/licenses/MPL-2.0)
