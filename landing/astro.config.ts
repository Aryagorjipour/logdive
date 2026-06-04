import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://aryagorjipour.github.io',
  base: '/logdive',
  output: 'static',
  build: {
    assets: '_assets',
  },
});
