// Structural edits to the builder's tree: add, remove, and drag-and-drop
// between groups.
import type { GroupState, NodeState } from './form';

export function findParent(g: GroupState, key: number): { parent: GroupState; index: number } | null {
	for (let i = 0; i < g.children.length; i++) {
		const c = g.children[i];
		if (c.key === key) return { parent: g, index: i };
		if (c.kind === 'group') {
			const r = findParent(c, key);
			if (r) return r;
		}
	}
	return null;
}

function contains(node: NodeState, key: number): boolean {
	if (node.key === key) return true;
	return node.kind === 'group' && node.children.some((c) => contains(c, key));
}

export function findGroup(g: GroupState, key: number): GroupState | null {
	if (g.key === key) return g;
	for (const c of g.children) {
		if (c.kind === 'group') {
			const r = findGroup(c, key);
			if (r) return r;
		}
	}
	return null;
}

export function removeNode(root: GroupState, key: number): NodeState | null {
	const at = findParent(root, key);
	if (!at) return null;
	return at.parent.children.splice(at.index, 1)[0];
}

export function insertAfter(root: GroupState, key: number, node: NodeState) {
	const at = findParent(root, key);
	if (at) at.parent.children.splice(at.index + 1, 0, node);
	else root.children.push(node);
}

/** Moves a node into a group at an index; never into itself or a descendant. */
export function moveNode(root: GroupState, key: number, targetGroup: number, index: number): boolean {
	const at = findParent(root, key);
	const target = findGroup(root, targetGroup);
	if (!at || !target) return false;
	if (contains(at.parent.children[at.index], targetGroup)) return false;
	const [node] = at.parent.children.splice(at.index, 1);
	const adjusted = at.parent === target && at.index < index ? index - 1 : index;
	target.children.splice(Math.min(adjusted, target.children.length), 0, node);
	return true;
}
