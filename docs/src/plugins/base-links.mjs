// Prefixes root-relative links in Markdown pages (`/usage/locking/`) with Astro's `base`, so pages can
// link to each other the same way whether the site is served from `/` or from a subpath like `/docs`.
export default function baseLinks(base = '/') {
	const prefix = base.replace(/\/+$/, '');
	return () => (tree) => {
		if (!prefix) return;
		const walk = (node) => {
			if (node.type === 'element' && node.tagName === 'a') {
				const href = node.properties?.href;
				if (
					typeof href === 'string' &&
					href.startsWith('/') &&
					!href.startsWith('//') &&
					href !== prefix &&
					!href.startsWith(`${prefix}/`)
				) {
					node.properties.href = prefix + href;
				}
			}
			node.children?.forEach(walk);
		};
		walk(tree);
	};
}
