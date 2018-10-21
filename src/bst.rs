
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
        let op = self.tree.mutate();
        if let Some(guard) = op.write_root() {
            Self::insert_to_node(guard, elem);
        } else {
            op.put_root_elem(elem);
        }
        /*
        match op.write_root() {
            Some(root_guard) => {
                Self::insert_to_node(root_guard, elem);
            },
            None => {
                op.put_root_elem(elem);
            },
        };
        */
    }

    fn insert_to_node<'tree: 'node, 'node>(mut guard: NodeWriteGuard<'tree, 'node, T, [ChildId; 2]>, elem: T) {
        let (node_elem, mut children) = guard.split();
        let child_branch =
            if elem > *node_elem {
                Some(1)
            } else if elem < *node_elem {
                Some(0)
            } else {
                None
            };
        if let Some(child_branch) = child_branch {
            match children.write_child(child_branch).unwrap() {
                Some(child_guard) => {
                    Self::insert_to_node(child_guard, elem);
                },
                None => {
                    children.put_child_elem(child_branch, elem).unwrap();
                }
            };
        }
    }

    pub fn remove(&mut self, elem: T) {
        let op = self.tree.mutate();
        /*
        if let Some(mut guard) = op.take_root() {
            if *guard.elem() != elem {
                if let Some(replacement) = Self::remove_from_node(guard, elem) {
                    op.put_root_tree(replacement);
                }
            }
        }
        */
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

    fn remove_from_node<'tree>(mut guard: NodeOwnedGuard<'tree, T, [ChildId; 2]>, elem: T)
        -> Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> {

        /*
        let (node_elem, mut children) = guard.split();
        let recurse_to =
            if elem > *node_elem {
                Some(1)
            } else if elem < *node_elem {
                Some(0)
            } else {
                None
            };
            */
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
            //let mut children = guard.children();
            //match (children.take_child(0).unwrap(), children.take_child(1).unwrap()) {
            match {
                let mut children = guard.split().1;
                //let tuple: (&mut T, ChildWriteGuard<'tree, _, T, [ChildId; 2]>) = guard.split();
                //let (elem, mut children) = tuple;
                //(children.take_child(0).unwrap(), children.take_child(1).unwrap())
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
                    if right_child.read().child(0).unwrap().is_none() &&
                        right_child.read().child(1).unwrap().is_none() {
                        // case: the right child is a leaf
                        // become the right child, and reattach the left child
                        right_child.split().1.put_child_tree(0, left_child).unwrap();
                        Some(right_child)
                    } else {
                        // case: the right child is a branch
                        // detach the smallest element in the right child
                        // become that element, and reattach both children
                        unimplemented!()
                    }
                }
            }
        }
    }

    fn detach_smallest<'tree, 'node: 'tree>(_guard: NodeWriteGuard<'tree, 'node, T, [ChildId; 2]>)
        -> Option<NodeOwnedGuard<'tree, T, [ChildId; 2]>> {

        unimplemented!()

    }
}


//#[test]
pub fn bst_test() {
    let mut tree: BinarySearchTree<i32> = BinarySearchTree::new();
    tree.insert(0);
    tree.insert(2);
    tree.insert(-1);
    tree.insert(-2);
    tree.insert(1);
    println!("{:#?}", tree);
}