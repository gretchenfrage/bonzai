
use super::*;

#[derive(Debug)]
pub struct BinarySearchTree<T: Ord + Eq> {
    tree: Tree<T, [ChildId; 2]>
}
impl<T: Ord + Eq> BinarySearchTree<T> {
    pub fn new() -> Self {
        BinarySearchTree {
            tree: Tree::new(),
        }
    }

    pub fn insert(&mut self, elem: T) {
        let op = self.tree.operation();
        {
            if let Some(guard) = op.write_root() {
                Self::insert_to_node(guard, elem);
            } else {
                op.put_root_elem(elem);
            }
        }
        op.finish_and_gc();
    }

    fn insert_to_node(mut guard: NodeWriteGuard<T, [ChildId; 2]>, elem: T) {
        let (node_elem, mut children) = guard.borrow_split();
        let child_branch =
            if elem > *node_elem {
                Some(1)
            } else if elem < *node_elem {
                Some(0)
            } else {
                None
            };
        if let Some(child_branch) = child_branch {
            let mut elem = Some(elem);
            // this would be so much nicer with NLL
            if match children.borrow_child_write(child_branch).unwrap() {
                Some(child_guard) => {
                    Self::insert_to_node(child_guard, elem.take().unwrap());
                    false
                },
                None => true,
            } {
                children.put_child_elem(child_branch, elem.take().unwrap()).unwrap();
            }
        }
    }

    pub fn remove(&mut self, elem: T) {
        let op = self.tree.operation();
        {
            let replacement =
                if let Some(mut guard) = op.take_root() {
                    if *guard.split().0 != elem {
                        Self::remove_from_node(guard, elem)
                    } else {
                        None
                    }
                } else {
                    None
                };
            if let Some(replacement) = replacement {
                op.put_root_tree(replacement);
            }
        }
        op.finish_and_gc();
    }

    fn remove_from_node<'tree>(mut guard: NodeOwnedGuard<'tree, T, [ChildId; 2]>, elem: T)
        -> Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> {

        let recurse_to = {
            let node_elem = guard.split().0;
            if elem > *node_elem {
                Some(1)
            } else if elem < *node_elem {
                Some(0)
            } else {
                None
            }
        };
        if let Some(child_branch) = recurse_to {
            // case: recursion
            {
                let mut children = guard.split().1;
                if let Some(child_guard) = children.take_child(child_branch).unwrap() {
                    if let Some(replacement) = Self::remove_from_node(child_guard, elem) {
                        children.put_child_tree(child_branch, replacement).unwrap();
                    }
                }
            }
            Some(guard)
        } else {

            // case: removal
            match {
                let (_, mut children) = guard.split();
                let left: Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> = children.take_child(0).unwrap();
                let right: Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> = children.take_child(1).unwrap();
                (left, right)
            } {
                (None, None) => {
                    // case: removal of leaf
                    // become nothing
                    None
                },
                (Some(left_child), None) => {
                    // case: removal of branch with left child
                    // become left child
                    Some(left_child)
                },
                (None, Some(right_child)) => {
                    // case: removal of branch with right child
                    // base right child
                    Some(right_child)
                },
                (Some(mut left_child), Some(mut right_child)) => {
                    // case: removal of branch with two children
                    if let Some(mut detached_smallest) = Self::detach_smallest(right_child.borrow()) {
                        // case: the right branch has some children, and we detached the
                        // leftmost child of the right branch
                        // become that child, and reattach the left and right branches
                        detached_smallest.children().put_child_tree(0, left_child).unwrap();
                        detached_smallest.children().put_child_tree(1, right_child).unwrap();
                        Some(detached_smallest)
                    } else {
                        // case: the right branch has no children
                        // become the right branch, and reattach the left branch
                        right_child.children().put_child_tree(0, left_child).unwrap();
                        Some(right_child)
                    }
                }
            }
        }
    }

    fn detach_smallest<'tree, 'node>(mut guard: NodeWriteGuard<'tree, 'node, T, [ChildId; 2]>)
        -> Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> {
        // if there is a leftmost branch, we can successfully detach smallest
        // otherwise, there are no children, so we fail
        guard.read()
            .child(0).unwrap()
            .map(|_| 0)
            .or_else(|| guard.read()
                .child(1).unwrap()
                .map(|_| 1))
            .map(|leftmost_branch| {
                // attempt to recurse on that branch
                Self::detach_smallest(guard.children().borrow_child_write(leftmost_branch).unwrap().unwrap())
                    .unwrap_or_else(|| {
                        // if this fails, then that branch is empty, so that's our base case
                        // we detach it
                        guard.children().take_child(leftmost_branch).unwrap().unwrap()
                    })
            })
    }
}

//#[test]
pub fn bst_test() {
    let mut tree: BinarySearchTree<i32> = BinarySearchTree::new();
    tree.insert(0);
    tree.insert(2);
    tree.insert(-1);

    //tree.remove(-1);

    tree.insert(-2);
    tree.insert(1);

    //tree.remove(1);

    println!("{:#?}", tree);
    println!();
    println!("{:#?}", tree.tree.debug_nodes());
}