
import path from 'path';

/** @type { import('@storybook/web-components-vite').StorybookConfig } */
const config = {
  stories: [
    '../dist/components/**/*.stories.mjs',
    '../dist/components/**/*.mdx',
    '../dist/docs/**/*.mdx',
  ],
  addons: ['@storybook/addon-links', '@storybook/addon-docs', './addon-fluent/index.cjs'],
  framework: {
    name: '@storybook/web-components-vite',
    options: {},
  },
  viteFinal: (config) => {
    // Ensure Vite can handle your assets and styles
    config.assetsInclude = config.assetsInclude || [];
    config.assetsInclude.push('**/*.svg', '**/*.ftl');
    
    // Handle CSS imports
    config.css = config.css || {};
    config.css.modules = false;
    
    config.resolve.alias = {
        ...config.resolve.alias,
        'lit.all.mjs': path.resolve(__dirname, '../dist/dependencies/lit.all.mjs'),
    };

    return config;
  },
  staticDirs: [
    { from: '../dist/assets', to: '/assets' },
    { from: '../dist/styles', to: '/styles' },
    { from: '../dist/dependencies', to: '/dependencies' },
  ],
};

export default config;