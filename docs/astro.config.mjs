// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import catppuccin from '@catppuccin/starlight';
import baseLinks from './src/plugins/base-links.mjs';

// Where the site is served. To move the docs later:
//   docs.imulup.com  → SITE = 'https://docs.imulup.com', BASE = '/'
//   imulup.com/docs  → SITE = 'https://imulup.com',      BASE = '/docs'
// Page links are written root-relative (`/usage/locking/`); `baseLinks` adds BASE to them.
const SITE = 'https://imulup.com';
const BASE = '/';

export default defineConfig({
	site: SITE,
	base: BASE,
	markdown: {
		rehypePlugins: [baseLinks(BASE)],
	},
	integrations: [
		starlight({
			title: 'imulup',
			description: 'Atlantis-style pull request automation for Pulumi.',
			favicon: '/favicon.svg',
			plugins: [catppuccin()],
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/imulup/imulup' }],
			editLink: { baseUrl: 'https://github.com/imulup/imulup/edit/main/docs/' },
			lastUpdated: true,
			sidebar: [
				{
					label: 'Start here',
					items: ['overview', 'quickstart'],
				},
				{
					label: 'Installation',
					items: ['install/docker', 'install/binary', 'install/production'],
				},
				{
					label: 'GitHub',
					items: ['github/app', 'github/token', 'github/enterprise'],
				},
				{
					label: 'Pulumi',
					items: ['pulumi/backends', 'pulumi/credentials', 'pulumi/stacks'],
				},
				{
					label: 'Usage',
					items: [
						'usage/pull-requests',
						'usage/projects',
						'usage/workflows',
						'usage/locking',
						'usage/apply-requirements',
						'usage/automerge',
					],
				},
				{
					label: 'Security',
					items: ['security/model', 'security/privilege-separation'],
				},
				{
					label: 'Reference',
					items: [
						'reference/server',
						'reference/repo-config',
						'reference/commands',
						'reference/step-environment',
						'reference/http',
					],
				},
				{
					label: 'Help',
					items: ['troubleshooting', 'faq', 'upgrading'],
				},
			],
		}),
	],
});
