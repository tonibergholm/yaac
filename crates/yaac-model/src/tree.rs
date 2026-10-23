use crate::{ContainerId, Node, NodeId, NodeKind, Position};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("node not found: {0}")]
    NotFound(NodeId),
    #[error("node already exists: {0}")]
    Duplicate(NodeId),
    #[error("move would create a cycle")]
    Cycle,
    #[error("a child's container must equal its parent's")]
    ContainerMismatch,
    #[error("parent is not a folder or split")]
    ParentNotContainer,
    #[error("a split holds two to four tab children and nothing else")]
    InvalidSplit,
    #[error("sibling position key space exhausted; renumber the siblings")]
    PositionExhausted,
}

/// What a move changed, so the caller can persist exactly those rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    /// The moved root and every descendant whose row changed (the root always; descendants only
    /// when `container_id` was rewritten).
    pub changed: Vec<NodeId>,
}

/// All nodes of a workspace, with the invariants from `docs/architecture.md` section 7 enforced on
/// every mutation: no cycles, a child's `container_id` equals its parent's, a split has two to four
/// tab children, siblings sort by (`position`, `id`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeState {
    pub(crate) nodes: BTreeMap<NodeId, Node>,
}

/// A split holds between these many tab children, inclusive.
const SPLIT_MIN: usize = 2;
const SPLIT_MAX: usize = 4;

impl TreeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a tree from stored rows, validating every invariant.
    pub fn from_nodes(nodes: impl IntoIterator<Item = Node>) -> Result<Self, TreeError> {
        let mut map = BTreeMap::new();
        for node in nodes {
            let id = node.id;
            if map.contains_key(&id) {
                return Err(TreeError::Duplicate(id));
            }
            map.insert(id, node);
        }
        let tree = Self { nodes: map };
        tree.validate()?;
        Ok(tree)
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Children of `parent` (or the container's top level when `None`), sorted by (position, id).
    pub fn children(&self, container: ContainerId, parent: Option<NodeId>) -> Vec<&Node> {
        let mut out: Vec<&Node> = self
            .nodes
            .values()
            .filter(|node| node.container_id == container && node.parent_id == parent)
            .collect();
        out.sort_by(|a, b| a.position.cmp(&b.position).then(a.id.cmp(&b.id)));
        out
    }

    /// The position for a new or moved node placed before `before` (or last when `None`) among the
    /// children of `parent` in `container`.
    ///
    /// Returns `PositionExhausted` when the key would be longer than 32 characters or when the
    /// neighbours tie. The caller then runs `renumber_children` and asks again. `before` must be a
    /// child of `parent` in `container`, otherwise this returns `NotFound(before)`.
    pub fn position_for(
        &self,
        container: ContainerId,
        parent: Option<NodeId>,
        before: Option<NodeId>,
    ) -> Result<Position, TreeError> {
        if let Some(parent) = parent {
            self.check_parent(container, parent)?;
        }
        let siblings = self.children(container, parent);
        let (lower, upper) = match before {
            None => (siblings.last().map(|node| &node.position), None),
            Some(id) => {
                let index = siblings
                    .iter()
                    .position(|node| node.id == id)
                    .ok_or(TreeError::NotFound(id))?;
                let lower = index.checked_sub(1).map(|prev| &siblings[prev].position);
                (lower, Some(&siblings[index].position))
            }
        };
        let key = Position::between(lower, upper).map_err(|_| TreeError::PositionExhausted)?;
        if key.needs_renumber() {
            return Err(TreeError::PositionExhausted);
        }
        Ok(key)
    }

    /// Inserts one node. A split cannot be inserted this way, because it needs two to four tab
    /// children at once; use `insert_split`. A node placed under a split must be a tab,
    /// and the split must have room for it.
    pub fn insert(&mut self, node: Node) -> Result<(), TreeError> {
        if self.nodes.contains_key(&node.id) {
            return Err(TreeError::Duplicate(node.id));
        }
        if let Some(parent) = node.parent_id {
            let parent_kind = self.check_parent(node.container_id, parent)?.kind;
            if parent_kind == NodeKind::Split {
                self.split_admits(parent, node.id, node.kind)?;
            }
        }
        if node.kind == NodeKind::Split {
            return Err(TreeError::InvalidSplit);
        }
        self.nodes.insert(node.id, node);
        Ok(())
    }

    /// Inserts a split and its tab children in one step. Each tab must have `parent_id` set to the
    /// split and the split's container. Either all nodes are inserted or none are.
    pub fn insert_split(&mut self, split: Node, tabs: Vec<Node>) -> Result<(), TreeError> {
        if split.kind != NodeKind::Split || !(SPLIT_MIN..=SPLIT_MAX).contains(&tabs.len()) {
            return Err(TreeError::InvalidSplit);
        }
        if self.nodes.contains_key(&split.id) {
            return Err(TreeError::Duplicate(split.id));
        }
        if let Some(parent) = split.parent_id
            && self.check_parent(split.container_id, parent)?.kind == NodeKind::Split
        {
            return Err(TreeError::InvalidSplit);
        }
        let mut seen = BTreeSet::from([split.id]);
        for tab in &tabs {
            if self.nodes.contains_key(&tab.id) || !seen.insert(tab.id) {
                return Err(TreeError::Duplicate(tab.id));
            }
            if tab.kind != NodeKind::Tab || tab.parent_id != Some(split.id) {
                return Err(TreeError::InvalidSplit);
            }
            if tab.container_id != split.container_id {
                return Err(TreeError::ContainerMismatch);
            }
        }
        for tab in tabs {
            self.nodes.insert(tab.id, tab);
        }
        self.nodes.insert(split.id, split);
        Ok(())
    }

    /// Removes the node and its whole subtree. Returns the removed nodes, root first.
    ///
    /// A split child cannot be removed alone when that would leave its split with fewer than two
    /// tabs: this returns `InvalidSplit`. Remove the whole split instead.
    pub fn remove_subtree(&mut self, id: NodeId) -> Result<Vec<Node>, TreeError> {
        let parent = self
            .nodes
            .get(&id)
            .ok_or(TreeError::NotFound(id))?
            .parent_id;
        if let Some(parent) = parent {
            self.check_split_keeps_minimum(parent)?;
        }
        let ids = self.subtree_ids(id);
        Ok(ids
            .into_iter()
            .filter_map(|removed| self.nodes.remove(&removed))
            .collect())
    }

    /// Moves the subtree rooted at `id` under `new_parent` in `new_container` at `new_position`.
    /// Rewrites `container_id` on every descendant when the container changes.
    ///
    /// A child leaves a split only while the split keeps two tabs (`InvalidSplit` otherwise). A
    /// child joins a split only as a tab, and only while the split has room. Cross-profile moves
    /// are refused by the core, because this model holds no container-to-profile map.
    pub fn move_node(
        &mut self,
        id: NodeId,
        new_container: ContainerId,
        new_parent: Option<NodeId>,
        new_position: Position,
    ) -> Result<MoveOutcome, TreeError> {
        let (old_container, old_parent, kind) = {
            let node = self.nodes.get(&id).ok_or(TreeError::NotFound(id))?;
            (node.container_id, node.parent_id, node.kind)
        };
        if let Some(parent) = new_parent {
            self.check_parent(new_container, parent)?;
            let mut cursor = Some(parent);
            while let Some(current) = cursor {
                if current == id {
                    return Err(TreeError::Cycle);
                }
                cursor = self.nodes.get(&current).and_then(|node| node.parent_id);
            }
            if self.is_split(parent) {
                self.split_admits(parent, id, kind)?;
            }
        }
        if let Some(old) = old_parent
            && new_parent != Some(old)
        {
            self.check_split_keeps_minimum(old)?;
        }

        let subtree = if new_container == old_container {
            vec![id]
        } else {
            self.subtree_ids(id)
        };
        for node_id in &subtree {
            if let Some(node) = self.nodes.get_mut(node_id) {
                node.container_id = new_container;
            }
        }
        if let Some(root) = self.nodes.get_mut(&id) {
            root.parent_id = new_parent;
            root.position = new_position;
        }
        Ok(MoveOutcome { changed: subtree })
    }

    /// Replaces every field of the stored node except its structure. `container_id`,
    /// `parent_id`, `position` and `kind` keep their stored values, whatever `node` carries. Use
    /// `move_node` to change structure.
    pub fn update(&mut self, node: Node) -> Result<(), TreeError> {
        let stored = self
            .nodes
            .get(&node.id)
            .ok_or(TreeError::NotFound(node.id))?;
        let mut next = node;
        next.container_id = stored.container_id;
        next.parent_id = stored.parent_id;
        next.position = stored.position.clone();
        next.kind = stored.kind;
        self.nodes.insert(next.id, next);
        Ok(())
    }

    /// Rewrites the positions of `parent`'s children in `container` to evenly spaced short keys,
    /// keeping their order and breaking any ties. Returns the ids whose position changed. Call it
    /// after `position_for` returns `PositionExhausted`. A missing parent has no children, so
    /// nothing changes.
    pub fn renumber_children(
        &mut self,
        container: ContainerId,
        parent: Option<NodeId>,
    ) -> Vec<NodeId> {
        let ids: Vec<NodeId> = self
            .children(container, parent)
            .iter()
            .map(|node| node.id)
            .collect();
        let keys = Position::evenly_spaced(ids.len());
        let mut changed = Vec::new();
        for (id, key) in ids.into_iter().zip(keys) {
            if let Some(node) = self.nodes.get_mut(&id).filter(|node| node.position != key) {
                node.position = key;
                changed.push(id);
            }
        }
        changed
    }

    /// Checks every invariant over the whole tree. Used after loading and in tests.
    pub fn validate(&self) -> Result<(), TreeError> {
        let mut split_children: BTreeMap<NodeId, usize> = BTreeMap::new();
        for node in self.nodes.values() {
            let Some(parent) = node.parent_id else {
                continue;
            };
            if self.check_parent(node.container_id, parent)?.kind == NodeKind::Split {
                if node.kind != NodeKind::Tab {
                    return Err(TreeError::InvalidSplit);
                }
                *split_children.entry(parent).or_insert(0) += 1;
            }
        }
        for node in self
            .nodes
            .values()
            .filter(|node| node.kind == NodeKind::Split)
        {
            let count = split_children.get(&node.id).copied().unwrap_or(0);
            if !(SPLIT_MIN..=SPLIT_MAX).contains(&count) {
                return Err(TreeError::InvalidSplit);
            }
        }
        self.check_acyclic()
    }

    /// The parent must exist, be in `container`, and be a folder or split.
    fn check_parent(&self, container: ContainerId, parent: NodeId) -> Result<&Node, TreeError> {
        let node = self.nodes.get(&parent).ok_or(TreeError::NotFound(parent))?;
        if node.container_id != container {
            return Err(TreeError::ContainerMismatch);
        }
        if !matches!(node.kind, NodeKind::Folder | NodeKind::Split) {
            return Err(TreeError::ParentNotContainer);
        }
        Ok(node)
    }

    /// `child` joins split `split`: it must be a tab and the split must have room, not counting
    /// `child` itself.
    fn split_admits(&self, split: NodeId, child: NodeId, kind: NodeKind) -> Result<(), TreeError> {
        let others = self
            .child_ids(split)
            .into_iter()
            .filter(|&id| id != child)
            .count();
        if kind != NodeKind::Tab || others >= SPLIT_MAX {
            return Err(TreeError::InvalidSplit);
        }
        Ok(())
    }

    /// One child is about to leave `parent`. Refuses when `parent` is a split with two children.
    fn check_split_keeps_minimum(&self, parent: NodeId) -> Result<(), TreeError> {
        if self.is_split(parent) && self.child_ids(parent).len() <= SPLIT_MIN {
            return Err(TreeError::InvalidSplit);
        }
        Ok(())
    }

    fn is_split(&self, id: NodeId) -> bool {
        self.nodes
            .get(&id)
            .is_some_and(|node| node.kind == NodeKind::Split)
    }

    fn child_ids(&self, parent: NodeId) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|node| node.parent_id == Some(parent))
            .map(|node| node.id)
            .collect()
    }

    /// `root` and all its descendants, root first.
    fn subtree_ids(&self, root: NodeId) -> Vec<NodeId> {
        let mut children: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for node in self.nodes.values() {
            if let Some(parent) = node.parent_id {
                children.entry(parent).or_default().push(node.id);
            }
        }
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            out.push(id);
            if let Some(kids) = children.get(&id) {
                stack.extend(kids.iter().copied());
            }
        }
        out
    }

    /// Every node must reach a root by following `parent_id`. Each node's chain is walked once
    /// and then marked as settled.
    fn check_acyclic(&self) -> Result<(), TreeError> {
        let mut settled: BTreeSet<NodeId> = BTreeSet::new();
        for &start in self.nodes.keys() {
            let mut path = BTreeSet::new();
            let mut cursor = Some(start);
            while let Some(id) = cursor {
                if settled.contains(&id) {
                    break;
                }
                if !path.insert(id) {
                    return Err(TreeError::Cycle);
                }
                cursor = self.nodes.get(&id).and_then(|node| node.parent_id);
            }
            settled.extend(path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeState;
    use proptest::prelude::*;
    use proptest::sample::Index;

    fn key(raw: &str) -> Position {
        Position::from_raw(raw.to_owned()).unwrap()
    }

    fn node(
        id: NodeId,
        container: ContainerId,
        parent: Option<NodeId>,
        kind: NodeKind,
        position: &str,
    ) -> Node {
        Node {
            id,
            container_id: container,
            parent_id: parent,
            position: key(position),
            kind,
            title: String::new(),
            url: None,
            state: NodeState::Suspended,
            is_unread: false,
            created_at: 0,
            last_active_at: 0,
            updated_at: 0,
            origin_device: None,
            rev: 0,
        }
    }

    fn tab(container: ContainerId, parent: Option<NodeId>, position: &str) -> Node {
        node(NodeId::new(), container, parent, NodeKind::Tab, position)
    }

    fn folder(container: ContainerId, parent: Option<NodeId>, position: &str) -> Node {
        node(NodeId::new(), container, parent, NodeKind::Folder, position)
    }

    fn split(container: ContainerId, parent: Option<NodeId>, position: &str) -> Node {
        node(NodeId::new(), container, parent, NodeKind::Split, position)
    }

    /// A split under `parent` with `count` fresh tab children.
    fn split_with_tabs(
        container: ContainerId,
        parent: Option<NodeId>,
        count: usize,
    ) -> (Node, Vec<Node>) {
        let split = split(container, parent, "V");
        let tabs = Position::evenly_spaced(count)
            .into_iter()
            .map(|position| {
                node(
                    NodeId::new(),
                    container,
                    Some(split.id),
                    NodeKind::Tab,
                    position.as_str(),
                )
            })
            .collect();
        (split, tabs)
    }

    fn tree_of(nodes: Vec<Node>) -> TreeState {
        TreeState::from_nodes(nodes).unwrap()
    }

    #[test]
    fn from_nodes_accepts_a_valid_tree_and_orders_children() {
        let c = ContainerId::new();
        let folder = folder(c, None, "V");
        let late = tab(c, Some(folder.id), "W");
        let early = tab(c, Some(folder.id), "B");
        let tree = tree_of(vec![late.clone(), folder.clone(), early.clone()]);
        let ids: Vec<NodeId> = tree
            .children(c, Some(folder.id))
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, vec![early.id, late.id]);
        assert_eq!(tree.len(), 3);
        assert!(!tree.is_empty());
        assert!(tree.get(folder.id).is_some());
        tree.validate().unwrap();
    }

    #[test]
    fn children_break_position_ties_by_id() {
        let c = ContainerId::new();
        let a = tab(c, None, "V");
        let b = tab(c, None, "V");
        let tree = tree_of(vec![a.clone(), b.clone()]);
        let mut expected = vec![a.id, b.id];
        expected.sort();
        let ids: Vec<NodeId> = tree.children(c, None).iter().map(|n| n.id).collect();
        assert_eq!(ids, expected);
    }

    #[test]
    fn from_nodes_rejects_duplicate_ids() {
        let c = ContainerId::new();
        let a = tab(c, None, "V");
        assert_eq!(
            TreeState::from_nodes(vec![a.clone(), a.clone()]),
            Err(TreeError::Duplicate(a.id))
        );
    }

    #[test]
    fn from_nodes_rejects_missing_parent() {
        let c = ContainerId::new();
        let missing = NodeId::new();
        let orphan = tab(c, Some(missing), "V");
        assert_eq!(
            TreeState::from_nodes(vec![orphan]),
            Err(TreeError::NotFound(missing))
        );
    }

    #[test]
    fn from_nodes_rejects_tab_parent() {
        let c = ContainerId::new();
        let t = tab(c, None, "V");
        let child = tab(c, Some(t.id), "V");
        assert_eq!(
            TreeState::from_nodes(vec![t, child]),
            Err(TreeError::ParentNotContainer)
        );
    }

    #[test]
    fn from_nodes_rejects_child_in_another_container() {
        let f = folder(ContainerId::new(), None, "V");
        let child = tab(ContainerId::new(), Some(f.id), "V");
        assert_eq!(
            TreeState::from_nodes(vec![f, child]),
            Err(TreeError::ContainerMismatch)
        );
    }

    #[test]
    fn from_nodes_rejects_cycles() {
        let c = ContainerId::new();
        let mut a = folder(c, None, "V");
        let b = folder(c, Some(a.id), "V");
        a.parent_id = Some(b.id);
        assert_eq!(TreeState::from_nodes(vec![a, b]), Err(TreeError::Cycle));

        let mut own = folder(c, None, "V");
        own.parent_id = Some(own.id);
        assert_eq!(TreeState::from_nodes(vec![own]), Err(TreeError::Cycle));
    }

    #[test]
    fn from_nodes_rejects_split_with_too_few_or_too_many_children() {
        let c = ContainerId::new();
        let (bare, _) = split_with_tabs(c, None, 2);
        assert_eq!(
            TreeState::from_nodes(vec![bare.clone()]),
            Err(TreeError::InvalidSplit)
        );

        let (s, tabs) = split_with_tabs(c, None, 1);
        let mut one = vec![s];
        one.extend(tabs);
        assert_eq!(TreeState::from_nodes(one), Err(TreeError::InvalidSplit));

        let (s, tabs) = split_with_tabs(c, None, 5);
        let mut five = vec![s];
        five.extend(tabs);
        assert_eq!(TreeState::from_nodes(five), Err(TreeError::InvalidSplit));
    }

    #[test]
    fn from_nodes_rejects_non_tab_child_of_split() {
        let c = ContainerId::new();
        let (s, mut tabs) = split_with_tabs(c, None, 2);
        tabs[0].kind = NodeKind::Folder;
        let mut all = vec![s];
        all.extend(tabs);
        assert_eq!(TreeState::from_nodes(all), Err(TreeError::InvalidSplit));
    }

    #[test]
    fn insert_adds_a_child_and_rejects_bad_parents() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, None, "V");
        let mut tree = tree_of(vec![f.clone(), t.clone()]);

        let child = tab(c, Some(f.id), "V");
        tree.insert(child.clone()).unwrap();
        assert_eq!(tree.get(child.id), Some(&child));

        assert_eq!(
            tree.insert(child.clone()),
            Err(TreeError::Duplicate(child.id))
        );
        let missing = NodeId::new();
        assert_eq!(
            tree.insert(tab(c, Some(missing), "V")),
            Err(TreeError::NotFound(missing))
        );
        assert_eq!(
            tree.insert(tab(c, Some(t.id), "V")),
            Err(TreeError::ParentNotContainer)
        );
        assert_eq!(
            tree.insert(tab(ContainerId::new(), Some(f.id), "V")),
            Err(TreeError::ContainerMismatch)
        );
        tree.validate().unwrap();
    }

    #[test]
    fn insert_refuses_a_bare_split() {
        let c = ContainerId::new();
        let mut tree = TreeState::new();
        assert_eq!(
            tree.insert(split(c, None, "V")),
            Err(TreeError::InvalidSplit)
        );
        assert!(tree.is_empty());
    }

    #[test]
    fn insert_checks_split_child_kind_and_capacity() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 4);
        let mut tree = {
            let mut all = vec![s.clone()];
            all.extend(tabs.clone());
            tree_of(all)
        };
        assert_eq!(
            tree.insert(tab(c, Some(s.id), "W")),
            Err(TreeError::InvalidSplit)
        );
        let folder_child = folder(c, Some(s.id), "W");
        assert_eq!(tree.insert(folder_child), Err(TreeError::InvalidSplit));
    }

    #[test]
    fn insert_split_is_atomic_and_checks_children() {
        let c = ContainerId::new();
        let mut tree = TreeState::new();
        let (s, tabs) = split_with_tabs(c, None, 2);
        tree.insert_split(s.clone(), tabs.clone()).unwrap();
        assert_eq!(tree.len(), 3);
        tree.validate().unwrap();

        let (s2, tabs2) = split_with_tabs(c, None, 3);
        let mut bad = tabs2.clone();
        bad[1].kind = NodeKind::Folder;
        assert_eq!(
            tree.insert_split(s2.clone(), bad),
            Err(TreeError::InvalidSplit)
        );
        assert_eq!(tree.len(), 3);

        assert_eq!(
            tree.insert_split(s2.clone(), tabs2[..1].to_vec()),
            Err(TreeError::InvalidSplit)
        );
        assert_eq!(
            tree.insert_split(s2.clone(), vec![tabs2[0].clone(), tabs2[0].clone()]),
            Err(TreeError::Duplicate(tabs2[0].id))
        );
        assert_eq!(
            tree.insert_split(tab(c, None, "V"), tabs2.clone()),
            Err(TreeError::InvalidSplit)
        );
        assert_eq!(tree.len(), 3);
        tree.validate().unwrap();
    }

    #[test]
    fn insert_split_rejects_bad_parents_and_wrong_containers() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, None, "V");
        let mut tree = tree_of(vec![f.clone(), t.clone()]);

        let (s, tabs) = split_with_tabs(c, Some(t.id), 2);
        assert_eq!(
            tree.insert_split(s, tabs),
            Err(TreeError::ParentNotContainer)
        );

        let (s, tabs) = split_with_tabs(c, Some(f.id), 2);
        let mut wrong = tabs.clone();
        wrong[0].container_id = ContainerId::new();
        assert_eq!(
            tree.insert_split(s.clone(), wrong),
            Err(TreeError::ContainerMismatch)
        );
        let mut stray = tabs.clone();
        stray[0].parent_id = None;
        assert_eq!(tree.insert_split(s, stray), Err(TreeError::InvalidSplit));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn insert_split_under_a_split_is_refused() {
        let c = ContainerId::new();
        let (outer, outer_tabs) = split_with_tabs(c, None, 2);
        let mut tree = {
            let mut all = vec![outer.clone()];
            all.extend(outer_tabs.clone());
            tree_of(all)
        };
        let (inner, inner_tabs) = split_with_tabs(c, Some(outer.id), 2);
        assert_eq!(
            tree.insert_split(inner, inner_tabs),
            Err(TreeError::InvalidSplit)
        );
    }

    #[test]
    fn remove_subtree_returns_root_first_and_removes_descendants() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let inner = folder(c, Some(f.id), "V");
        let leaf = tab(c, Some(inner.id), "V");
        let other = tab(c, None, "W");
        let mut tree = tree_of(vec![f.clone(), inner.clone(), leaf.clone(), other.clone()]);

        let removed = tree.remove_subtree(f.id).unwrap();
        let ids: Vec<NodeId> = removed.iter().map(|n| n.id).collect();
        assert_eq!(ids[0], f.id);
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&inner.id) && ids.contains(&leaf.id));
        assert_eq!(tree.len(), 1);
        assert!(tree.get(other.id).is_some());
        tree.validate().unwrap();
    }

    #[test]
    fn remove_subtree_reports_missing_node() {
        let mut tree = TreeState::new();
        let missing = NodeId::new();
        assert_eq!(
            tree.remove_subtree(missing),
            Err(TreeError::NotFound(missing))
        );
    }

    #[test]
    fn remove_split_child_refused_at_two_and_allowed_at_three() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 2);
        let mut all = vec![s.clone()];
        all.extend(tabs.clone());
        let mut tree = tree_of(all);
        assert_eq!(
            tree.remove_subtree(tabs[0].id),
            Err(TreeError::InvalidSplit)
        );
        assert_eq!(tree.len(), 3);

        let (s3, tabs3) = split_with_tabs(c, None, 3);
        let mut all3 = vec![s3.clone()];
        all3.extend(tabs3.clone());
        let mut tree3 = tree_of(all3);
        let removed = tree3.remove_subtree(tabs3[0].id).unwrap();
        assert_eq!(removed.len(), 1);
        tree3.validate().unwrap();
    }

    #[test]
    fn remove_whole_split_is_allowed() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 2);
        let mut all = vec![s.clone()];
        all.extend(tabs.clone());
        let mut tree = tree_of(all);
        let removed = tree.remove_subtree(s.id).unwrap();
        assert_eq!(removed.len(), 3);
        assert!(tree.is_empty());
    }

    #[test]
    fn move_within_one_container_changes_only_the_root() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, Some(f.id), "V");
        let mut tree = tree_of(vec![f.clone(), t.clone()]);
        let outcome = tree.move_node(t.id, c, None, key("A")).unwrap();
        assert_eq!(outcome.changed, vec![t.id]);
        let moved = tree.get(t.id).unwrap();
        assert_eq!(moved.parent_id, None);
        assert_eq!(moved.position.as_str(), "A");
        tree.validate().unwrap();
    }

    #[test]
    fn move_across_containers_rewrites_every_descendant() {
        let c1 = ContainerId::new();
        let c2 = ContainerId::new();
        let f = folder(c1, None, "V");
        let inner = folder(c1, Some(f.id), "V");
        let leaf = tab(c1, Some(inner.id), "V");
        let bystander = tab(c1, None, "W");
        let mut tree = tree_of(vec![
            f.clone(),
            inner.clone(),
            leaf.clone(),
            bystander.clone(),
        ]);

        let outcome = tree.move_node(f.id, c2, None, key("V")).unwrap();
        let mut changed = outcome.changed.clone();
        changed.sort();
        let mut expected = vec![f.id, inner.id, leaf.id];
        expected.sort();
        assert_eq!(changed, expected);
        for id in [f.id, inner.id, leaf.id] {
            assert_eq!(tree.get(id).unwrap().container_id, c2);
        }
        assert_eq!(tree.get(bystander.id).unwrap().container_id, c1);
        tree.validate().unwrap();
    }

    #[test]
    fn move_under_a_parent_in_its_own_subtree_is_a_cycle() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let inner = folder(c, Some(f.id), "V");
        let mut tree = tree_of(vec![f.clone(), inner.clone()]);
        assert_eq!(
            tree.move_node(f.id, c, Some(inner.id), key("V")),
            Err(TreeError::Cycle)
        );
        assert_eq!(
            tree.move_node(f.id, c, Some(f.id), key("V")),
            Err(TreeError::Cycle)
        );
        assert_eq!(tree.get(f.id).unwrap().parent_id, None);
    }

    #[test]
    fn move_reports_missing_node_and_parent_and_bad_parents() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, None, "V");
        let mut tree = tree_of(vec![f.clone(), t.clone()]);
        let missing = NodeId::new();
        assert_eq!(
            tree.move_node(missing, c, None, key("V")),
            Err(TreeError::NotFound(missing))
        );
        assert_eq!(
            tree.move_node(t.id, c, Some(missing), key("V")),
            Err(TreeError::NotFound(missing))
        );
        assert_eq!(
            tree.move_node(t.id, c, Some(t.id), key("V")),
            Err(TreeError::ParentNotContainer)
        );
        assert_eq!(
            tree.move_node(t.id, ContainerId::new(), Some(f.id), key("V")),
            Err(TreeError::ContainerMismatch)
        );
        tree.validate().unwrap();
    }

    #[test]
    fn move_folder_into_split_is_refused() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 2);
        let f = folder(c, None, "V");
        let mut all = vec![s.clone(), f.clone()];
        all.extend(tabs);
        let mut tree = tree_of(all);
        assert_eq!(
            tree.move_node(f.id, c, Some(s.id), key("V")),
            Err(TreeError::InvalidSplit)
        );
    }

    #[test]
    fn move_into_full_split_is_refused_and_within_split_is_allowed() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 4);
        let outsider = tab(c, None, "W");
        let mut all = vec![s.clone(), outsider.clone()];
        all.extend(tabs.clone());
        let mut tree = tree_of(all);
        assert_eq!(
            tree.move_node(outsider.id, c, Some(s.id), key("V")),
            Err(TreeError::InvalidSplit)
        );
        let outcome = tree.move_node(tabs[0].id, c, Some(s.id), key("Z")).unwrap();
        assert_eq!(outcome.changed, vec![tabs[0].id]);
        tree.validate().unwrap();
    }

    #[test]
    fn move_out_of_split_below_two_is_refused() {
        let c = ContainerId::new();
        let (s, tabs) = split_with_tabs(c, None, 2);
        let mut all = vec![s.clone()];
        all.extend(tabs.clone());
        let mut tree = tree_of(all);
        assert_eq!(
            tree.move_node(tabs[0].id, c, None, key("Z")),
            Err(TreeError::InvalidSplit)
        );
        tree.validate().unwrap();
    }

    #[test]
    fn move_tab_between_splits_respects_both_limits() {
        let c = ContainerId::new();
        let (a, a_tabs) = split_with_tabs(c, None, 3);
        let (b, b_tabs) = split_with_tabs(c, None, 2);
        let mut all = vec![a.clone(), b.clone()];
        all.extend(a_tabs.clone());
        all.extend(b_tabs.clone());
        let mut tree = tree_of(all);
        tree.move_node(a_tabs[0].id, c, Some(b.id), key("Z"))
            .unwrap();
        assert_eq!(tree.get(a_tabs[0].id).unwrap().parent_id, Some(b.id));
        tree.validate().unwrap();
        // A now holds two tabs, so one more leaving A is refused.
        assert_eq!(
            tree.move_node(a_tabs[1].id, c, Some(b.id), key("Z")),
            Err(TreeError::InvalidSplit)
        );
    }

    #[test]
    fn split_child_can_leave_a_split_of_three_across_containers() {
        let c1 = ContainerId::new();
        let c2 = ContainerId::new();
        let (s, tabs) = split_with_tabs(c1, None, 3);
        let mut all = vec![s.clone()];
        all.extend(tabs.clone());
        let mut tree = tree_of(all);
        let outcome = tree.move_node(tabs[0].id, c2, None, key("Z")).unwrap();
        assert_eq!(outcome.changed, vec![tabs[0].id]);
        assert_eq!(tree.children(c1, Some(s.id)).len(), 2);
        assert_eq!(tree.get(tabs[0].id).unwrap().container_id, c2);
        tree.validate().unwrap();
    }

    #[test]
    fn update_replaces_mutable_fields_and_keeps_structure() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, Some(f.id), "V");
        let mut tree = tree_of(vec![f.clone(), t.clone()]);

        let mut edited = t.clone();
        edited.title = "Renamed".to_owned();
        edited.url = Some("https://example.org".to_owned());
        edited.is_unread = true;
        edited.parent_id = None;
        edited.position = key("Q");
        edited.container_id = ContainerId::new();
        tree.update(edited).unwrap();

        let stored = tree.get(t.id).unwrap();
        assert_eq!(stored.title, "Renamed");
        assert_eq!(stored.url.as_deref(), Some("https://example.org"));
        assert!(stored.is_unread);
        assert_eq!(stored.parent_id, Some(f.id));
        assert_eq!(stored.position, t.position);
        assert_eq!(stored.container_id, c);
        tree.validate().unwrap();

        let missing = tab(c, None, "V");
        assert_eq!(
            tree.update(missing.clone()),
            Err(TreeError::NotFound(missing.id))
        );
    }

    #[test]
    fn position_for_places_before_and_after_siblings() {
        let c = ContainerId::new();
        let a = tab(c, None, "B");
        let b = tab(c, None, "D");
        let tree = tree_of(vec![a.clone(), b.clone()]);

        let append = tree.position_for(c, None, None).unwrap();
        assert!(append > b.position);

        let between = tree.position_for(c, None, Some(b.id)).unwrap();
        assert!(a.position < between && between < b.position);

        let first = tree.position_for(c, None, Some(a.id)).unwrap();
        assert!(first < a.position);

        let empty = TreeState::new();
        assert_eq!(
            empty.position_for(c, None, None).unwrap(),
            Position::first()
        );
    }

    #[test]
    fn position_for_rejects_bad_targets() {
        let c = ContainerId::new();
        let f = folder(c, None, "V");
        let t = tab(c, None, "V");
        let inside = tab(c, Some(f.id), "V");
        let tree = tree_of(vec![f.clone(), t.clone(), inside.clone()]);

        let missing = NodeId::new();
        assert_eq!(
            tree.position_for(c, Some(missing), None),
            Err(TreeError::NotFound(missing))
        );
        assert_eq!(
            tree.position_for(c, Some(t.id), None),
            Err(TreeError::ParentNotContainer)
        );
        assert_eq!(
            tree.position_for(ContainerId::new(), Some(f.id), None),
            Err(TreeError::ContainerMismatch)
        );
        assert_eq!(
            tree.position_for(c, None, Some(inside.id)),
            Err(TreeError::NotFound(inside.id))
        );
    }

    #[test]
    fn position_for_reports_exhaustion_for_ties() {
        let c = ContainerId::new();
        let a = tab(c, None, "V");
        let b = tab(c, None, "V");
        let tree = tree_of(vec![a, b.clone()]);
        assert_eq!(
            tree.position_for(c, None, Some(b.id)),
            Err(TreeError::PositionExhausted)
        );
    }

    #[test]
    fn position_for_reports_exhaustion_for_long_keys_and_renumber_recovers() {
        let c = ContainerId::new();
        let lower = format!("V{}", "z".repeat(31));
        let a = tab(c, None, &lower);
        let b = tab(c, None, "W");
        let mut tree = tree_of(vec![a.clone(), b.clone()]);

        assert_eq!(
            tree.position_for(c, None, Some(b.id)),
            Err(TreeError::PositionExhausted)
        );
        let changed = tree.renumber_children(c, None);
        assert!(!changed.is_empty());
        let order: Vec<NodeId> = tree.children(c, None).iter().map(|n| n.id).collect();
        assert_eq!(order, vec![a.id, b.id]);
        let key = tree.position_for(c, None, Some(b.id)).unwrap();
        assert!(!key.needs_renumber());
        assert!(tree.get(a.id).unwrap().position < key && key < tree.get(b.id).unwrap().position);
        assert!(tree.renumber_children(c, None).is_empty());
    }

    #[test]
    fn renumber_children_breaks_ties_and_keeps_order() {
        let c = ContainerId::new();
        let a = tab(c, None, "V");
        let b = tab(c, None, "V");
        let mut tree = tree_of(vec![a.clone(), b.clone()]);
        let before: Vec<NodeId> = tree.children(c, None).iter().map(|n| n.id).collect();
        let changed = tree.renumber_children(c, None);
        let after: Vec<NodeId> = tree.children(c, None).iter().map(|n| n.id).collect();
        assert_eq!(before, after);
        assert!(!changed.is_empty());
        assert_ne!(
            tree.get(a.id).unwrap().position,
            tree.get(b.id).unwrap().position
        );
        tree.validate().unwrap();
        assert!(tree.renumber_children(c, None).is_empty());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// Insertion at one spot never fails. A long sibling makes exhaustion happen at once, and
        /// every later key stays within 32 characters because exhaustion triggers a renumber.
        #[test]
        fn repeated_insertion_at_one_spot_renumbers_and_keeps_keys_short(
            before_first in any::<bool>(),
            rounds in 100usize..200,
        ) {
            let c = ContainerId::new();
            let long = format!("V{}", "z".repeat(31));
            let first = tab(c, None, &long);
            let second = tab(c, None, "W");
            let mut tree = tree_of(vec![first.clone(), second.clone()]);
            let anchor = if before_first { first.id } else { second.id };
            let mut renumbered = false;
            for _ in 0..rounds {
                let position = match tree.position_for(c, None, Some(anchor)) {
                    Ok(position) => position,
                    Err(TreeError::PositionExhausted) => {
                        renumbered = true;
                        tree.renumber_children(c, None);
                        tree.position_for(c, None, Some(anchor))
                            .map_err(|error| TestCaseError::fail(error.to_string()))?
                    }
                    Err(error) => return Err(TestCaseError::fail(error.to_string())),
                };
                prop_assert!(position.as_str().len() <= 32);
                tree.insert(node(NodeId::new(), c, None, NodeKind::Tab, position.as_str()))
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
            }
            prop_assert!(renumbered);
            prop_assert_eq!(tree.len(), rounds + 2);
            tree.validate().map_err(|error| TestCaseError::fail(error.to_string()))?;
        }
    }

    /// One random operation. `Index` values resolve against the tree's current contents when the
    /// step runs, so every step targets real nodes or the top level.
    #[derive(Debug, Clone)]
    enum Step {
        Insert {
            parent: Index,
            tab: bool,
            before: Index,
            container: Index,
        },
        InsertSplit {
            parent: Index,
            before: Index,
            container: Index,
            tabs: usize,
        },
        Move {
            node: Index,
            parent: Index,
            before: Index,
            container: Index,
        },
        Remove {
            node: Index,
        },
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (
                any::<Index>(),
                any::<bool>(),
                any::<Index>(),
                any::<Index>()
            )
                .prop_map(|(parent, tab, before, container)| Step::Insert {
                    parent,
                    tab,
                    before,
                    container
                }),
            (any::<Index>(), any::<Index>(), any::<Index>(), 2usize..=4).prop_map(
                |(parent, before, container, tabs)| Step::InsertSplit {
                    parent,
                    before,
                    container,
                    tabs
                }
            ),
            (
                any::<Index>(),
                any::<Index>(),
                any::<Index>(),
                any::<Index>()
            )
                .prop_map(|(node, parent, before, container)| Step::Move {
                    node,
                    parent,
                    before,
                    container
                }),
            any::<Index>().prop_map(|node| Step::Remove { node }),
        ]
    }

    /// `None` is one of `len + 1` choices, so the top level stays reachable.
    fn pick_optional(ids: &[NodeId], index: Index) -> Option<NodeId> {
        ids.get(index.index(ids.len() + 1)).copied()
    }

    fn pick_required(ids: &[NodeId], index: Index) -> Option<NodeId> {
        if ids.is_empty() {
            return None;
        }
        ids.get(index.index(ids.len())).copied()
    }

    fn container_for(
        tree: &TreeState,
        containers: &[ContainerId],
        parent: Option<NodeId>,
        pick: Index,
    ) -> ContainerId {
        match parent.and_then(|id| tree.get(id)) {
            Some(node) => node.container_id,
            None => containers[pick.index(containers.len())],
        }
    }

    fn parent_is_split(tree: &TreeState, parent: Option<NodeId>) -> bool {
        parent
            .and_then(|id| tree.get(id))
            .is_some_and(|node| node.kind == NodeKind::Split)
    }

    fn siblings_of(
        tree: &TreeState,
        container: ContainerId,
        parent: Option<NodeId>,
    ) -> Vec<NodeId> {
        tree.children(container, parent)
            .iter()
            .map(|node| node.id)
            .collect()
    }

    /// The ids of `root` and its descendants, found by walking each node's ancestor chain. This
    /// is independent of the tree's own subtree walk.
    fn subtree_of(tree: &TreeState, root: NodeId) -> BTreeSet<NodeId> {
        tree.nodes
            .keys()
            .copied()
            .filter(|&id| {
                let mut cursor = Some(id);
                while let Some(current) = cursor {
                    if current == root {
                        return true;
                    }
                    cursor = tree.nodes.get(&current).and_then(|node| node.parent_id);
                }
                false
            })
            .collect()
    }

    fn position_with_renumber(
        tree: &mut TreeState,
        container: ContainerId,
        parent: Option<NodeId>,
        before: Option<NodeId>,
    ) -> Result<Position, TreeError> {
        match tree.position_for(container, parent, before) {
            Err(TreeError::PositionExhausted) => {
                tree.renumber_children(container, parent);
                tree.position_for(container, parent, before)
            }
            other => other,
        }
    }

    /// Validates the tree and checks that its ids are exactly the prior ids plus `added` minus
    /// `removed`. Nothing is lost or duplicated.
    fn check_ok(
        tree: &TreeState,
        prepared: &TreeState,
        added: &BTreeSet<NodeId>,
        removed: &BTreeSet<NodeId>,
    ) -> Result<(), TestCaseError> {
        tree.validate()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut expected: BTreeSet<NodeId> = prepared.nodes.keys().copied().collect();
        expected.extend(added.iter().copied());
        for id in removed {
            expected.remove(id);
        }
        let actual: BTreeSet<NodeId> = tree.nodes.keys().copied().collect();
        prop_assert_eq!(actual, expected);
        Ok(())
    }

    /// A tree with a folder, a two-tab split, a top-level tab and a second container's folder.
    fn seeded() -> (Vec<ContainerId>, TreeState) {
        let containers: Vec<ContainerId> = (0..3).map(|_| ContainerId::new()).collect();
        let (c0, c1) = (containers[0], containers[1]);
        let f0 = folder(c0, None, "V");
        let (split_node, tabs) = split_with_tabs(c0, Some(f0.id), 2);
        let top = tab(c0, None, "W");
        let g = folder(c1, None, "V");
        let inner = tab(c1, Some(g.id), "V");
        let mut all = vec![f0, top, g, inner, split_node];
        all.extend(tabs);
        (containers, tree_of(all))
    }

    fn apply(
        tree: &mut TreeState,
        containers: &[ContainerId],
        step: &Step,
    ) -> Result<(), TestCaseError> {
        let ids: Vec<NodeId> = tree.nodes.keys().copied().collect();
        match step {
            Step::Insert {
                parent,
                tab: as_tab,
                before,
                container,
            } => {
                let parent = pick_optional(&ids, *parent);
                let container = container_for(tree, containers, parent, *container);
                let kind = if *as_tab || parent_is_split(tree, parent) {
                    NodeKind::Tab
                } else {
                    NodeKind::Folder
                };
                let before = pick_optional(&siblings_of(tree, container, parent), *before);
                let prepared_before_renumber = tree.clone();
                let Ok(position) = position_with_renumber(tree, container, parent, before) else {
                    prop_assert!(*tree == prepared_before_renumber);
                    return Ok(());
                };
                let prepared = tree.clone();
                let new_node = node(NodeId::new(), container, parent, kind, position.as_str());
                let new_id = new_node.id;
                match tree.insert(new_node) {
                    Ok(()) => {
                        check_ok(tree, &prepared, &BTreeSet::from([new_id]), &BTreeSet::new())?
                    }
                    Err(_) => prop_assert!(*tree == prepared),
                }
            }
            Step::InsertSplit {
                parent,
                before,
                container,
                tabs,
            } => {
                let parent = pick_optional(&ids, *parent);
                let container = container_for(tree, containers, parent, *container);
                let before = pick_optional(&siblings_of(tree, container, parent), *before);
                let prepared_before_renumber = tree.clone();
                let Ok(position) = position_with_renumber(tree, container, parent, before) else {
                    prop_assert!(*tree == prepared_before_renumber);
                    return Ok(());
                };
                let prepared = tree.clone();
                let split_node = node(
                    NodeId::new(),
                    container,
                    parent,
                    NodeKind::Split,
                    position.as_str(),
                );
                let split_id = split_node.id;
                let tab_nodes: Vec<Node> = Position::evenly_spaced(*tabs)
                    .iter()
                    .map(|tab_position| {
                        node(
                            NodeId::new(),
                            container,
                            Some(split_id),
                            NodeKind::Tab,
                            tab_position.as_str(),
                        )
                    })
                    .collect();
                let added: BTreeSet<NodeId> = std::iter::once(split_id)
                    .chain(tab_nodes.iter().map(|tab_node| tab_node.id))
                    .collect();
                match tree.insert_split(split_node, tab_nodes) {
                    Ok(()) => check_ok(tree, &prepared, &added, &BTreeSet::new())?,
                    Err(_) => prop_assert!(*tree == prepared),
                }
            }
            Step::Move {
                node: node_pick,
                parent,
                before,
                container,
            } => {
                let Some(moving) = pick_required(&ids, *node_pick) else {
                    return Ok(());
                };
                let parent = pick_optional(&ids, *parent);
                let container = container_for(tree, containers, parent, *container);
                let before = pick_optional(&siblings_of(tree, container, parent), *before);
                let prepared_before_renumber = tree.clone();
                let Ok(position) = position_with_renumber(tree, container, parent, before) else {
                    prop_assert!(*tree == prepared_before_renumber);
                    return Ok(());
                };
                let prepared = tree.clone();
                let subtree = subtree_of(&prepared, moving);
                let container_changes = prepared
                    .get(moving)
                    .is_some_and(|node| node.container_id != container);
                match tree.move_node(moving, container, parent, position.clone()) {
                    Ok(outcome) => {
                        check_ok(tree, &prepared, &BTreeSet::new(), &BTreeSet::new())?;
                        let changed: BTreeSet<NodeId> = outcome.changed.iter().copied().collect();
                        let expected_changed = if container_changes {
                            subtree.clone()
                        } else {
                            BTreeSet::from([moving])
                        };
                        prop_assert_eq!(changed, expected_changed);
                        for id in &subtree {
                            prop_assert_eq!(
                                tree.get(*id).map(|node| node.container_id),
                                Some(container)
                            );
                        }
                        for (id, stored) in &prepared.nodes {
                            if !subtree.contains(id) {
                                prop_assert!(tree.get(*id) == Some(stored));
                            }
                        }
                        prop_assert_eq!(tree.get(moving).map(|node| node.parent_id), Some(parent));
                        prop_assert_eq!(
                            tree.get(moving).map(|node| node.position.clone()),
                            Some(position)
                        );
                    }
                    Err(_) => prop_assert!(*tree == prepared),
                }
            }
            Step::Remove { node: node_pick } => {
                let Some(target) = pick_required(&ids, *node_pick) else {
                    return Ok(());
                };
                let prepared = tree.clone();
                let subtree = subtree_of(&prepared, target);
                match tree.remove_subtree(target) {
                    Ok(removed) => {
                        let removed_ids: BTreeSet<NodeId> =
                            removed.iter().map(|node| node.id).collect();
                        prop_assert_eq!(removed_ids.clone(), subtree);
                        prop_assert_eq!(removed.first().map(|node| node.id), Some(target));
                        check_ok(tree, &prepared, &BTreeSet::new(), &removed_ids)?;
                    }
                    Err(_) => prop_assert!(*tree == prepared),
                }
            }
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn random_steps_keep_invariants_and_failed_steps_change_nothing(
            steps in prop::collection::vec(step(), 1..40),
        ) {
            let (containers, mut tree) = seeded();
            tree.validate().map_err(|error| TestCaseError::fail(error.to_string()))?;
            for step in &steps {
                apply(&mut tree, &containers, step)?;
            }
        }
    }
}
