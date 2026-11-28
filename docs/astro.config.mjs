// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import catppuccin from '@catppuccin/starlight';

export default defineConfig({
	site: 'https://docs.imulup.com',
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
