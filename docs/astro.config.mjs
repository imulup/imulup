// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// Published to GitHub Pages at https://imulup.github.io/imulup/. If you move the site to a custom
// domain, change `site`, drop `base`, and replace the `/imulup/` prefix in page links.
export default defineConfig({
	site: 'https://imulup.github.io',
	base: '/imulup',
	integrations: [
		starlight({
			title: 'imulup',
			description: 'Atlantis-style pull request automation for Pulumi.',
			favicon: '/favicon.svg',
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
