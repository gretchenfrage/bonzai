
#![feature(fixed_size_array)]

extern crate core;

use core::array::FixedSizeArray;
use std::cell::UnsafeCell;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ChildId {
    index: Option<usize>
}

struct Node<T, C: FixedSizeArray<ChildId>> {
    data: UnsafeCell<T>,
    parent: Option<usize>,
    children: C,
}

/// A tree, stored in a constant number of contiguous memory blocks, in order to increase
/// cache performance. Generic over element type and branch factor.
pub struct Tree<T, C: FixedSizeArray<ChildId>> {
    nodes: Vec<Option<Node<T, C>>>,
    root: Option<usize>,
    garbage: Vec<usize>,
}
impl<T, C: FixedSizeArray<ChildId>> Tree<T, C> {
    /// Construct a new, empty tree.
    pub fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
            garbage: Vec::new(),
        }
    }

    /// Remove nodes which are marked as garbage, which requires changing indices to maintain
    /// the integrity of parent/child relationships.
    pub fn compact(&mut self) {
        unimplemented!()
    }

    pub fn root(&self) -> OptionNodeGuard<T, C> {
        unimplemented!()
    }

}

/// A guard for immutably accessing a node and its children, even if the node does not exist.
pub struct OptionNodeGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: Option<&'tree Node<T, C>>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> OptionNodeGuard<'tree, T, C> {
    pub fn as_option(&self) -> Option<NodeGuard<'tree, T, C>> {
        self.node.map(|node| NodeGuard {
            tree: self.tree,
            node
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct InvalidChildIndex(pub usize);

/// A guard for immutably accessing a node and its children.
pub struct NodeGuard<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree Tree<T, C>,
    node: &'tree Node<T, C>,
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeGuard<'tree, T, C> {
    /// Get the tree this node belongs to.
    pub fn tree(&self) -> &'tree Tree<T, C> {
        self.tree
    }

    /// Get this node's data.
    pub fn data(&self) -> &'tree T {
        unsafe {
            &*self.node.data.get()
        }
    }

    /// Get a child of this node in the form of an `OptionNodeGuard`.
    pub fn child(&self, index: usize) -> Result<OptionNodeGuard<'tree, T, C>, InvalidChildIndex> {
        self.node.children.as_slice()
            .get(index)
            .ok_or(InvalidChildIndex(index))
            .map(|child_id| OptionNodeGuard {
                tree: self.tree,
                node: child_id.index
                    .map(|i| (&self.tree.nodes[i])
                        .as_ref()
                        .unwrap())
            })
    }
}

/// A guard for mutably accessing a node and its children, even if the node does not exist.
pub struct OptionNodeGuardMut<'tree, T, C: FixedSizeArray<ChildId>> {
    tree: &'tree mut Tree<T, C>,
    node: Option<&'tree mut Node<T, C>>
}
impl<'tree, T, C: FixedSizeArray<ChildId>> OptionNodeGuardMut<'tree, T, C> {
    pub fn as_option<'s>(&'s mut self) -> Option<NodeGuardMut<'s, T, C>> {
        /*
        self.node.clone().map(|node| NodeGuardMut {
            tree: self.tree,
            node
        })
        */
        /*
        let (tree, option_node) = (self.tree, self.node);
        option_node.map(|node| NodeGuardMut {
            tree,
            node,
        })
        */
        match self {
            &mut OptionNodeGuardMut {
                ref mut tree,
                node: Some(ref mut node)
            } => Some(NodeGuardMut {
                tree,
                node,
            }),
            &mut OptionNodeGuardMut {
                node: None,
                ..
            } => None
        }
    }
}

/// A guard for mutable accessing a node and its children.
pub struct NodeGuardMut<'tree, T, C: FixedSizeArray<ChildId>> {
    // TODO: of course, you can't have both a reference to the tree and a reference to a node
    // TODO: within the tree at the same time. we need to put them both behind an unsafecell
    tree: &'tree mut Tree<T, C>,
    node: &'tree mut Node<T, C>
}
impl<'tree, T, C: FixedSizeArray<ChildId>> NodeGuardMut<'tree, T, C> {
    /// Get the tree this node belongs to.
    pub fn tree(&mut self) -> &mut Tree<T, C> {
        self.tree
    }

    /// Get this node's data.
    pub fn data(&mut self) -> &mut T {
        unsafe {
            &mut*self.node.data.get()
        }
    }

    /// Get a child of this node in the form of an `OptionNodeGuardMut`.
    pub fn child<'s>(&'s mut self, index: usize) -> Result<OptionNodeGuardMut<'s, T, C>, InvalidChildIndex> {
        let &mut NodeGuardMut {
            ref mut tree,
            ref mut node,
        } = self;
        node.children.as_slice()
            .get(index)
            .ok_or(InvalidChildIndex(index))
            .map(move |child_id| OptionNodeGuardMut {
                tree,
                node: child_id.index
                    .map(|i| (&mut tree.nodes[i])
                        .as_mut()
                        .unwrap())
            })
    }
}


// TODO: if a node is marked as garbage, then all its children should be garbage collected
// TODO: implement debug

fn main() {
    println!("Hello, world!");
}
